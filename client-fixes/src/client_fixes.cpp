#include <windows.h>

namespace {

DWORD WINAPI ShowLoadedConsole(LPVOID) {
    if (!AllocConsole()) {
        OutputDebugStringW(L"SP Client Fixes loaded, but could not open its console.\n");
        return 0;
    }

    SetConsoleTitleW(L"SP Client Fixes");
    constexpr wchar_t message[] =
        L"SP Client Fixes DLL loaded.\r\n"
        L"No gameplay fixes are active yet.\r\n"
        L"Close the game to close this log window.\r\n";
    DWORD written = 0;
    WriteConsoleW(GetStdHandle(STD_OUTPUT_HANDLE), message,
                  static_cast<DWORD>(sizeof(message) / sizeof(message[0]) - 1),
                  &written, nullptr);
    return 0;
}

}  // namespace

extern "C" __declspec(dllexport) unsigned int SPClientFixesVersion() {
    return 1;
}

// The launcher loads this DLL only when Client fixes is enabled. Keep DllMain
// minimal; the console and later fixes initialise on a worker thread after
// the loader callback returns. Do not wait for that thread here.
BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(module);
        HANDLE thread = CreateThread(nullptr, 0, ShowLoadedConsole, nullptr, 0, nullptr);
        if (thread) {
            CloseHandle(thread);
        } else {
            OutputDebugStringW(L"SP Client Fixes loaded, but could not start its log thread.\n");
        }
    }
    return TRUE;
}
