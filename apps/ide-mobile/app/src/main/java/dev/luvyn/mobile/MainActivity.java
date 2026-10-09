package dev.luvyn.mobile;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.webkit.*;
import androidx.webkit.WebViewAssetLoader;
import androidx.documentfile.provider.DocumentFile;
import org.json.JSONObject;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.concurrent.Executors;

/** Platform boundary: provider URIs never enter the compiler. Documents are staged in private storage. */
public final class MainActivity extends Activity {
    private static final String ORIGIN = "https://appassets.androidplatform.net";
    private WebView web;
    private File docs, target;
    private DocumentFile docsTree, targetTree;
    private final java.util.concurrent.ExecutorService worker = Executors.newSingleThreadExecutor();
    private String driveToken;
    private String authorizationId, authorizationInput;
    private String pendingProjectName;
    private long importBytes;
    private int importFiles;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        // Restore the selected provider and its private staging directory across activity/process recreation.
        String selected = getPreferences(0).getString("docs", null);
        docs = new File(getFilesDir(), getPreferences(0).getString("staging", "documentation"));
        target = new File(getFilesDir(), "target"); docs.mkdirs(); target.mkdirs();
        if (selected != null) { try { docsTree = providerProject(Uri.parse(selected)); } catch (Exception ignored) { docsTree = null; } }
        String destination = getPreferences(0).getString("target", null);
        if (destination != null) targetTree = DocumentFile.fromTreeUri(this, Uri.parse(destination));
        web = new WebView(this);
        android.widget.FrameLayout container = new android.widget.FrameLayout(this);
        container.addView(web, new android.widget.FrameLayout.LayoutParams(-1, -1));
        container.setOnApplyWindowInsetsListener((view, insets) -> {
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = insets.getInsets(android.view.WindowInsets.Type.systemBars() | android.view.WindowInsets.Type.displayCutout() | android.view.WindowInsets.Type.ime());
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom);
            } else { view.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(), insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom()); }
            return insets;
        });
        setContentView(container);
        web.getSettings().setJavaScriptEnabled(true);
        web.getSettings().setDomStorageEnabled(true);
        web.getSettings().setAllowFileAccess(false);
        web.getSettings().setAllowContentAccess(false);
        web.getSettings().setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        web.setWebChromeClient(new WebChromeClient());
        WebViewAssetLoader loader = new WebViewAssetLoader.Builder().addPathHandler("/", new WebViewAssetLoader.AssetsPathHandler(this)).build();
        web.setWebViewClient(new WebViewClient() {
            @Override public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                WebResourceResponse local = loader.shouldInterceptRequest(request.getUrl());
                if (local != null) return local;
                return new WebResourceResponse("text/plain", "UTF-8", new ByteArrayInputStream(new byte[0]));
            }
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) { return !request.getUrl().toString().startsWith(ORIGIN + "/"); }
        });
        web.addJavascriptInterface(new NativeInterface(), "LuvynNative");
        web.loadUrl(ORIGIN + "/mobile.html");
    }
    public final class NativeInterface {
        @JavascriptInterface public void request(String id, String input) { worker.execute(() -> dispatch(id, input)); }
    }
    private void reply(String id, String json) {
        runOnUiThread(() -> web.evaluateJavascript("window.luvynReply(" + JSONObject.quote(id) + "," + JSONObject.quote(json) + ")", null));
    }
    private void dispatch(String id, String input) {
        try {
            JSONObject request = new JSONObject(input); String op = request.getString("op");
            request.put("_storage", new File(getFilesDir(), "projects").getAbsolutePath());
            if (op.equals("drive-connect") || ((op.startsWith("drive-") || op.equals("project-sync")) && !request.has("_authorized"))) {
                authorizeDrive(id, request.toString()); return;
            }
            if (driveToken != null) request.put("_token", driveToken);
            if (op.equals("project-create")) {
                String name = request.getString("name").trim();
                if (name.isEmpty() || name.length() > 160 || name.contains("/") || name.contains("\\") || name.equals("..")) throw new IOException("Nome de projeto inválido");
                pendingProjectName = name;
                runOnUiThread(() -> startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION), 4));
                reply(id, "{\"pending\":true}"); return;
            }
            if (op.equals("project-open")) {
                String path = request.getString("path");
                if (path.startsWith("content://")) { openProvider(Uri.parse(path), null); }
                else { File project = new File(path).getCanonicalFile(); if (!project.toPath().startsWith(getFilesDir().getCanonicalFile().toPath()) || !project.isDirectory()) throw new IOException("Projeto indisponível"); docs = project; docsTree = null; remember(project.getName(), project.getAbsolutePath()); CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"reload\"}"); }
                reply(id, projects().toString()); return;
            }
            if (op.equals("open-docs") || op.equals("open-target")) {
                final int pickerCode = op.equals("open-docs") ? 1 : 2;
                runOnUiThread(() -> startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION), pickerCode));
                reply(id, "{\"ok\":true}"); return;
            }
            if (op.equals("platform-info")) { reply(id, info().toString()); return; }
            if (op.equals("build") && targetTree == null && docsTree != null) {
                runOnUiThread(() -> startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION), 2));
                reply(id, "{\"pending\":true,\"target_required\":true}");
                return;
            }
            // Compare provider content with the disk version before overwriting a saved document.
            if (op.equals("save") && docsTree != null) {
                String path = request.getString("file"); DocumentFile remote = document(docsTree, path, false, false);
                File staged = local(path);
                if (remote == null || !read(remote).equals(new String(Files.readAllBytes(staged.toPath()), StandardCharsets.UTF_8))) throw new IOException("Arquivo alterado no provider. Reimporte para revisar antes de salvar.");
            }
            if (op.equals("reload-provider")) {
                if (docsTree != null) { File fresh = new File(getFilesDir(), "docs-refresh-" + System.nanoTime()); importBytes = 0; importFiles = 0; stage(docsTree, fresh, 0); docs = fresh; getPreferences(0).edit().putString("staging", fresh.getName()).apply(); }
                request.put("op", "reload"); op = "reload";
            }
            String result = CoreBridge.request(docs.getAbsolutePath(), target.getAbsolutePath(), request.toString());
            JSONObject value = new JSONObject(result);
            if (!value.has("error")) {
                if (docsTree != null && (op.equals("save") || op.equals("create"))) {
                    String path = request.getString("file");
                    DocumentFile remote = document(docsTree, path, true, false);
                    write(remote, Files.readAllBytes(local(path).toPath()));
                } else if (docsTree != null && op.equals("delete")) {
                    DocumentFile remote = document(docsTree, request.getString("file"), false, false);
                    if (remote != null && !remote.delete()) throw new IOException("Provider recusou exclusão; reimporte para reconciliar.");
                } else if (docsTree != null && op.equals("move")) {
                    DocumentFile from = document(docsTree, request.getString("file"), false, false);
                    DocumentFile to = document(docsTree, request.getString("to"), true, false);
                    write(to, Files.readAllBytes(local(request.getString("to")).toPath()));
                    if (from != null && !from.delete()) throw new IOException("Provider recusou remoção após move; revise ambas as cópias.");
                }
                if (op.equals("build") && targetTree != null) {
                    String output = value.getString("output_relative").replace('\\', '/');
                    File artifact = new File(target, output).getCanonicalFile();
                    if (!artifact.toPath().startsWith(target.getCanonicalFile().toPath())) throw new IOException("Invalid artifact path");
                    write(document(targetTree, output, true, false), Files.readAllBytes(artifact.toPath()));
                    value.put("output", targetTree.getUri() + "/" + output);
                }
                if (op.equals("snapshot")) { value.put("documentation_root", docsTree == null ? docs.getAbsolutePath() : docsTree.getUri().toString()); value.put("target_project_root", targetTree == null ? "Selecione Target" : targetTree.getUri().toString()); }
            }
            if (!value.has("error") && value.has("_workspace")) {
                docs = new File(value.getString("_workspace")); docsTree = null;
                getPreferences(0).edit().remove("docs").putString("staging", getFilesDir().toPath().relativize(docs.toPath()).toString()).apply();
                CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"reload\"}");
                value.remove("_workspace");
            }
            if (op.equals("projects") || op.equals("project-forget")) value = projects();
            reply(id, value.toString());
        } catch (Exception error) { reply(id, errorJson(error)); }
    }
    private String errorJson(Exception error) { return "{\"error\":" + JSONObject.quote(error.getMessage() == null ? error.toString() : error.getMessage()) + "}"; }
    private JSONObject info() throws org.json.JSONException { return new JSONObject().put("documentation", docsTree == null ? docs.getAbsolutePath() : docsTree.getUri().toString()).put("target", targetTree == null ? "Não selecionado" : targetTree.getUri().toString()); }
    private File local(String path) throws IOException {
        File file = new File(docs, path).getCanonicalFile();
        if (!file.toPath().startsWith(docs.getCanonicalFile().toPath()) || path.contains("..") || path.startsWith("/")) throw new IOException("Invalid workspace path");
        return file;
    }
    private DocumentFile document(DocumentFile root, String path, boolean create, boolean directory) throws IOException {
        if (root == null || path.startsWith("/") || path.contains("..")) throw new IOException("Invalid provider path");
        String[] parts = path.split("/"); DocumentFile current = root;
        for (int i = 0; i < parts.length; i++) {
            DocumentFile next = current.findFile(parts[i]);
            boolean folder = i < parts.length - 1 || directory;
            if (next == null && create) next = folder ? current.createDirectory(parts[i]) : current.createFile("application/octet-stream", parts[i]);
            if (next == null) return null;
            current = next;
        }
        return current;
    }
    private String read(DocumentFile file) throws IOException {
        try (InputStream input = getContentResolver().openInputStream(file.getUri())) {
            if (input == null) throw new IOException("Provider não abriu arquivo");
            ByteArrayOutputStream buffer = new ByteArrayOutputStream(); byte[] chunk = new byte[8192]; int count;
            while ((count = input.read(chunk)) >= 0) { buffer.write(chunk, 0, count); if (buffer.size() > 2 * 1024 * 1024) throw new IOException("Documento excede 2 MiB"); }
            byte[] data = buffer.toByteArray();
            if (data.length > 2 * 1024 * 1024) throw new IOException("Documento excede 2 MiB");
            return StandardCharsets.UTF_8.newDecoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(data)).toString();
        }
    }
    private void write(DocumentFile file, byte[] data) throws IOException {
        if (file == null) throw new IOException("Provider não criou arquivo");
        try (OutputStream output = getContentResolver().openOutputStream(file.getUri(), "wt")) {
            if (output == null) throw new IOException("Provider não abriu destino"); output.write(data);
        }
    }
    private void stage(DocumentFile tree, File destination, int depth) throws IOException {
        if (depth > 32) throw new IOException("Árvore excede profundidade 32");
        destination.mkdirs();
        for (DocumentFile child : tree.listFiles()) {
            String name = child.getName();
            if (name == null || name.contains("/") || name.contains("\\") || name.equals("..") || name.equals(".git") || name.equals(".luvyn") || name.equals("target") || name.equals("node_modules") || name.equals("dist") || name.equals("graphify-out")) continue;
            File local = new File(destination, name);
            if (child.isDirectory()) stage(child, local, depth + 1);
            else if (name.endsWith(".lyn") || name.equals("luvyn.toml") || name.equals(".ignore.luvyn") || name.equals(".luvynignore") || name.equals(".gitignore")) {
                String text = read(child); importBytes += text.getBytes(StandardCharsets.UTF_8).length;
                if (++importFiles > 10000 || importBytes > 64 * 1024 * 1024) throw new IOException("Import excede 10.000 documentos ou 64 MiB");
                Files.write(local.toPath(), text.getBytes(StandardCharsets.UTF_8));
            }
        }
    }
    @Override protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        if (code == 3) {
            if (result != RESULT_OK || data == null) { reply(authorizationId, "{\"error\":\"Google authorization cancelled\"}"); authorizationId=null; return; }
            try { finishAuthorization(com.google.android.gms.auth.api.identity.Identity.getAuthorizationClient(this).getAuthorizationResultFromIntent(data)); }
            catch (Exception error) { reply(authorizationId, errorJson(error)); authorizationId=null; } return;
        }
        if (result != RESULT_OK || data == null || data.getData() == null) return;
        Uri uri = data.getData();
        try { getContentResolver().takePersistableUriPermission(uri, data.getFlags() & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION)); }
        catch (SecurityException error) { reply("event", errorJson(error)); return; }
        worker.execute(() -> {
            try {
                DocumentFile tree = DocumentFile.fromTreeUri(this, uri);
                if (tree == null) throw new IOException("Provider indisponível");
                if (code == 1 || code == 4) {
                    openProvider(uri, code == 4 ? pendingProjectName : null);
                } else { targetTree = tree; getPreferences(0).edit().putString("target", uri.toString()).apply(); }
                CoreBridge.request(docs.getAbsolutePath(), target.getAbsolutePath(), "{\"op\":\"reload\"}");
                reply("event", "{\"workspaceChanged\":true}");
            } catch (Exception error) { reply("event", errorJson(error)); }
        });
    }
    private void remember(String name, String path) throws Exception {
        JSONObject request = new JSONObject().put("op", "project-remember").put("name", name).put("path", path).put("_storage", new File(getFilesDir(),"projects").getAbsolutePath());
        JSONObject result = new JSONObject(CoreBridge.request(docs.getAbsolutePath(), target.getAbsolutePath(), request.toString()));
        if (result.has("error")) throw new IOException(result.getString("error"));
    }
    private JSONObject projects() throws Exception {
        JSONObject request = new JSONObject().put("op","projects").put("_storage",new File(getFilesDir(),"projects").getAbsolutePath());
        if (driveToken != null) request.put("_token",driveToken);
        JSONObject value = new JSONObject(CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),request.toString()));
        org.json.JSONArray recent = value.optJSONArray("recent");
        if (recent != null) for (int i=0;i<recent.length();i++) { JSONObject item=recent.getJSONObject(i); String path=item.getString("path"); if (path.startsWith("content://")) { DocumentFile tree; try {tree=providerProject(Uri.parse(path));} catch(Exception ignored){tree=null;} item.put("unavailable",tree==null||!tree.exists()||!tree.canRead()); } }
        return value;
    }
    private DocumentFile providerProject(Uri uri) throws IOException {
        DocumentFile tree=DocumentFile.fromTreeUri(this,uri.buildUpon().fragment(null).build());
        if(uri.getFragment()!=null) {tree=tree==null?null:tree.findFile(uri.getFragment());}
        return tree;
    }
    private void openProvider(Uri uri, String createName) throws Exception {
        DocumentFile tree=providerProject(uri);
        if (tree==null || !tree.canRead()) throw new IOException("Provider indisponível");
        if (createName != null) {
            if (tree.findFile(createName)!=null) throw new IOException("Projeto já existe");
            tree=tree.createDirectory(createName); if(tree==null)throw new IOException("Provider recusou criação");
            write(tree.createFile("text/plain","main.lyn"),"main:\n    purpose:\n        descrever o projeto\n".getBytes(StandardCharsets.UTF_8));
            write(tree.createFile("text/plain","luvyn.toml"),"sources = [\".\"]\n".getBytes(StandardCharsets.UTF_8));
        }
        File fresh=new File(getFilesDir(),"docs-"+Integer.toHexString(tree.getUri().toString().hashCode())+"-"+System.nanoTime());
        importBytes=0;importFiles=0;stage(tree,fresh,0);docs=fresh;docsTree=tree;
        getPreferences(0).edit().putString("docs",tree.getUri().toString()).putString("staging",fresh.getName()).apply();
        Uri projectUri=createName==null?uri:uri.buildUpon().fragment(createName).build();
        getPreferences(0).edit().putString("docs",projectUri.toString()).apply();
        remember(tree.getName()==null?"Project":tree.getName(),projectUri.toString());
        CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"reload\"}");
    }
    private void authorizeDrive(String id, String input) {
        runOnUiThread(() -> {
            if (authorizationId != null) { reply(id,"{\"error\":\"Google authorization already pending\"}"); return; }
            authorizationId=id;authorizationInput=input;
            com.google.android.gms.auth.api.identity.AuthorizationRequest request=com.google.android.gms.auth.api.identity.AuthorizationRequest.builder().setRequestedScopes(java.util.Collections.singletonList(new com.google.android.gms.common.api.Scope("https://www.googleapis.com/auth/drive.file"))).build();
            com.google.android.gms.auth.api.identity.Identity.getAuthorizationClient(this).authorize(request).addOnSuccessListener(result -> {
                if(result.hasResolution()) { try {startIntentSenderForResult(result.getPendingIntent().getIntentSender(),3,null,0,0,0);}catch(Exception error){reply(authorizationId,errorJson(error));authorizationId=null;} }
                else finishAuthorization(result);
            }).addOnFailureListener(error -> {reply(authorizationId,errorJson(error));authorizationId=null;});
        });
    }
    private void finishAuthorization(com.google.android.gms.auth.api.identity.AuthorizationResult result) {
        String id=authorizationId,input=authorizationInput;authorizationId=null;authorizationInput=null;
        driveToken=result.getAccessToken();
        if(driveToken==null){reply(id,"{\"error\":\"Google did not return an access token\"}");return;}
        try { JSONObject request=new JSONObject(input); if(request.getString("op").equals("drive-connect")){reply(id,"{\"connected\":true}");reply("event","{\"authorizationConnected\":true}");}else{request.put("_authorized",true);worker.execute(()->dispatch(id,request.toString()));} }
        catch(Exception error){reply(id,errorJson(error));}
    }
    @Override protected void onDestroy() { web.removeJavascriptInterface("LuvynNative"); web.destroy(); worker.shutdown(); super.onDestroy(); }
}
