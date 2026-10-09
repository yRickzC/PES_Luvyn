package dev.luvyn.mobile;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.core.app.ActivityScenario;
import androidx.test.platform.app.InstrumentationRegistry;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.json.JSONObject;
import java.io.File;
import java.nio.file.Files;
import java.nio.charset.StandardCharsets;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public final class CoreIntegrationTest {
    @Test public void nativeCoreBuildsSeparatedWorkspaceAndDictionaryOnAndroid() throws Exception {
        File base = new File(InstrumentationRegistry.getInstrumentation().getTargetContext().getCacheDir(), "core-test-" + System.nanoTime());
        File docs = new File(base, "documentation"), target = new File(base, "target");
        assertTrue(docs.mkdirs()); assertTrue(target.mkdirs());
        String text = "@entity\nclass Player\nfields:\n    health: f32\nrules:\n    - self.health >= 0.0\nfunc damage(amount: f32) -> Result<(), DamageError>\nsource: src/player.rs::damage\n@error\nclass DamageError\n";
        Files.write(new File(docs,"player.lyn").toPath(), text.getBytes(StandardCharsets.UTF_8));
        JSONObject dictionary = new JSONObject(CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"language\"}"));
        assertFalse(dictionary.has("error")); assertTrue(dictionary.getJSONArray("entries").toString().contains("bool"));
        JSONObject build = new JSONObject(CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"build\"}"));
        assertFalse(build.toString(), build.has("error")); assertTrue(new File(target,".luvyn/project.lu").isFile());
        assertEquals(".luvyn/project.lu", build.getString("output_relative"));
        assertFalse(new File(docs,".luvyn/project.lu").exists());
        JSONObject graph = new JSONObject(CoreBridge.request(docs.getAbsolutePath(),target.getAbsolutePath(),"{\"op\":\"graph\",\"query\":\"player\",\"depth\":1}"));
        assertFalse(graph.toString(), graph.has("error")); assertTrue(graph.toString().contains("module"));
    }
    @Test public void mobileActivityStartsWithLocalWebView() {
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> assertNotNull(activity.findViewById(android.R.id.content)));
        }
    }
}
