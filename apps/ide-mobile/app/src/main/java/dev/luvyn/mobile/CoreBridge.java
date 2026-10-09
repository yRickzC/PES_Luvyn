package dev.luvyn.mobile;
final class CoreBridge {
    static { System.loadLibrary("luvyn_mobile"); }
    static native String request(String documentationRoot, String targetRoot, String request);
}
