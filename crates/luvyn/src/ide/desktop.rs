//! Native window host. All editor/semantic operations still use the shared local backend.
use crate::ide::host::Target;
use luvyn_core::{Error, Result};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder};

struct Backend {
    shutdown: Option<tokio::sync::watch::Sender<bool>>,
    worker: Option<std::thread::JoinHandle<Result<()>>>,
}
impl Backend {
    fn stop(&mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| Error::Message("Desktop backend thread panicked".into()))??;
        }
        Ok(())
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn start_backend(workspace: PathBuf, port: u16) -> Result<(String, Backend, Arc<AtomicBool>)> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let backend_root = workspace.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let worker = std::thread::spawn(move || -> Result<()> {
        let result = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?
            .block_on(crate::server::run_host(
                backend_root,
                true,
                port,
                Target::Desktop,
                Some(ready_tx),
            ));
        worker_finished.store(true, Ordering::Release);
        result
    });
    let (url, shutdown) = match ready_rx.recv_timeout(Duration::from_secs(25)) {
        Ok(ready) => ready,
        Err(e) => {
            let _ = worker.join();
            return Err(Error::Message(format!("Desktop backend startup: {e}")));
        }
    };
    Ok((
        url,
        Backend {
            shutdown: Some(shutdown),
            worker: Some(worker),
        },
        finished,
    ))
}

pub fn run(workspace: PathBuf, port: u16) -> Result<()> {
    let (url, mut backend, finished) = start_backend(workspace.clone(), port)?;
    let close_backend = backend.shutdown.as_ref().unwrap().clone();
    let mut event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(format!(
            "Luvyn — {}",
            if luvyn_core::projects::is_launcher(&workspace) {
                "Projects".into()
            } else {
                workspace.file_name().unwrap_or_default().to_string_lossy()
            }
        ))
        .with_inner_size(tao::dpi::LogicalSize::new(1440.0, 920.0))
        .build(&event_loop)
        .map_err(|e| Error::Message(format!("Desktop window: {e}")))?;
    let profile = luvyn_core::Project::open(&workspace)?.safe_path(".luvyn/webview")?;
    std::fs::create_dir_all(&profile)?;
    let mut context = WebContext::new(Some(profile));
    let builder = WebViewBuilder::new_with_web_context(&mut context).with_url(&url);
    #[cfg(not(target_os = "linux"))]
    let webview = builder
        .build(&window)
        .map_err(|e| Error::Message(format!("Native WebView runtime: {e}")))?;
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder
            .build_gtk(window.gtk_window())
            .map_err(|e| Error::Message(format!("Native WebView runtime: {e}")))?
    };
    event_loop.run_return(move |event, _, flow| {
        let _ = &webview; // Retain the WebView for the entire native event loop.
        let _ = &context;
        *flow = ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(150));
        if matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        ) || finished.load(Ordering::Acquire)
        {
            let _ = close_backend.send(true);
            *flow = ControlFlow::Exit;
        }
    });
    backend.stop()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_backend_binds_dynamically_and_stops_with_its_owner() {
        let workspace = tempfile::tempdir().unwrap();
        let (url, mut backend, _) = start_backend(workspace.path().to_owned(), 0).unwrap();
        let endpoint = url
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let address: std::net::SocketAddr = endpoint.parse().unwrap();
        let connection = std::net::TcpStream::connect(address).unwrap();
        drop(connection);
        backend.stop().unwrap();
        assert!(
            std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err()
        );
    }
}
