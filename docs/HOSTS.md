# Hosts e distribuição

`luvyn-core::ide` contém a sessão compartilhada, overlays, workspace, editor e ações. Server (Axum) e Desktop (Wry/Tao) transportam o mesmo protocolo JSON; Mobile (Android WebView/JNI) usa a mesma sessão Rust e uma UI própria de toque. Nenhum host implementa parser, tipos ou graph separadamente.

- `luvyn ide . --server`: browser e servidor local no terminal; Ctrl+C ou fechar o terminal encerra o servidor.
- `luvyn ide . --desktop`: janela nativa com servidor pertencente à janela; fechar a janela encerra o backend. Usa porta dinâmica por padrão e não grava instância Desktop reutilizável.
- `luvyn ide build --server` / `--desktop`: npm/Vite + Cargo release, assets embutidos em `.luvyn/dist/<target>`.
- `luvyn ide build --android`: Rust Android + frontend compartilhado + Gradle, APK em `dist/android/luvyn-debug.apk`.

Distribuições exigem repositório fonte ou `--source`. `--output` escolhe diretório de entrega. Builds Android exigem SDK 36, build-tools 36.0.0, NDK 27+, JDK 17+, target Rust Android e acesso à rede no primeiro build. Gradle Wrapper 9.6.1 e AGP 9.2.1 estão fixados no projeto. ARM64 é default; `LUVYN_ANDROID_ABI=x86_64` permite emulator. Bibliotecas nativas usam alinhamento de 16 KiB.

Mobile carrega somente assets locais via WebViewAssetLoader; navegação externa é bloqueada. Bridge assíncrona executa Core e provider IO fora da UI. SAF preserva concessões de diretórios. Documents são importados em staging privado; Target é selecionado separadamente. Paths/URIs Android não entram no Core. A configuração de target desktop é preservada no arquivo, mas o host fornece seu target privado e publica o `.lu` via provider.

Build de documentação continua sendo `luvyn build`, diferente do build de distribuição. CLI/Desktop/Server seguem `project.target` de `luvyn.toml`. Cache fica no projeto de documentação; lock/artifact ficam no target. Mobile usa o mesmo compiler/build sobre staging e copia somente artefato ao Target SAF.

Desktop: WebView2 no Windows, WebKit no macOS, GTK3/WebKitGTK 4.1 no Linux. O backend Desktop pertence à janela e termina quando ela fecha. Server escuta apenas em loopback, sem token de sessão ou validação de `Origin`; sem daemon, registro de PID ou processos separados.

Referências de plataforma: [SAF](https://developer.android.com/training/data-storage/shared/documents-files), [WebViewAssetLoader](https://developer.android.com/develop/ui/views/layout/webapps/load-local-content), [AGP 9.2](https://developer.android.com/build/releases/agp-9-2-0-release-notes).

## Seleção de projetos

Sem workspace, `luvyn ide --server` / `--desktop` abre Projetos. O Desktop é o host padrão: `luvyn ide` abre a janela nativa, e `--server` seleciona explicitamente o navegador. Mobile sempre começa por Projetos. Server pertence ao terminal; comandos status/stop foram removidos. Locais e Cloud passam pela mesma sessão após abrir. Veja [Projects](PROJECTS.md) para persistência, OAuth e sync.
