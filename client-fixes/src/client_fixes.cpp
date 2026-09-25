#include <windows.h>

extern "C" __declspec(dllexport) unsigned int SPClientFixesVersion() {
    return 1;
}

// The launcher loads this DLL only when Client fixes is enabled. Keep DllMain
// minimal; later fixes should initialise on a worker thread after checking the
// exact game build and the current Unreal world. No hooks are active yet.
BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(module);
    }
    return TRUE;
}
