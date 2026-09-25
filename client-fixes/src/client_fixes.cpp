#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <cstdint>
#include <cwchar>
#include <vector>

namespace {
// BravoHotel 1.3.0.473797 only. Never use these offsets on another binary.
constexpr wchar_t kSha256[] = L"16b8b421371457d936e5cc1810ff707b5f5984126973dbdc4e6b5c714522051f";
constexpr std::uintptr_t kObjectsRva = 0x762f708;
constexpr std::uintptr_t kTableRva = 0x6b3c0f8;
constexpr std::uint64_t kPointerXor = 0xba146ab1e38e3211;
constexpr LONG kClassLevel = 5;

// Write diagnostics to both the DLL console and a debugger, if attached.
void Log(const wchar_t* message) {
    OutputDebugStringW(message);
    HANDLE handle = GetStdHandle(STD_OUTPUT_HANDLE);
    if (handle && handle != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        WriteConsoleW(handle, message, static_cast<DWORD>(wcslen(message)), &written, nullptr);
    }
}

// Read game memory without crashing if an object disappears during inspection.
template<class T> bool Read(std::uintptr_t address, T& value) {
    SIZE_T count = 0;
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address),
                                        &value, sizeof(T), &count) && count == sizeof(T);
}
// Read an entire GObjects chunk or pointer substitution table.
bool ReadBlock(std::uintptr_t address, void* data, SIZE_T size) {
    SIZE_T count = 0;
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address),
                                        data, size, &count) && count == size;
}
// Change an aligned level field only if it still has the expected value.
// Catch access violations if the game destroys the object between checks.
bool ChangeLevel(std::uintptr_t address, LONG before, LONG after) {
    __try {
        return InterlockedCompareExchange(reinterpret_cast<volatile LONG*>(address),
                                          after, before) == before;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// Hash the running executable and reject every build except the researched one.
bool SupportedBuild() {
    std::vector<wchar_t> path(32768);
    DWORD length = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
    if (!length || length >= path.size()) return false;
    HANDLE file = CreateFileW(path.data(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                              nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    DWORD objectSize = 0, returned = 0;
    std::array<UCHAR, 32> digest{};
    bool okay = false;
    if (BCRYPT_SUCCESS(BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0)) &&
        BCRYPT_SUCCESS(BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                       reinterpret_cast<PUCHAR>(&objectSize), sizeof(objectSize), &returned, 0)) &&
        objectSize && objectSize < 4096) {
        std::vector<UCHAR> hashStorage(objectSize);
        if (BCRYPT_SUCCESS(BCryptCreateHash(algorithm, &hash, hashStorage.data(),
                                           objectSize, nullptr, 0, 0))) {
            std::array<UCHAR, 65536> buffer{};
            bool complete = true;
            for (;;) {
                DWORD got = 0;
                if (!ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &got, nullptr)) {
                    complete = false; break;
                }
                if (!got) break;
                if (!BCRYPT_SUCCESS(BCryptHashData(hash, buffer.data(), got, 0))) {
                    complete = false; break;
                }
            }
            if (complete && BCRYPT_SUCCESS(BCryptFinishHash(hash, digest.data(),
                                                             static_cast<ULONG>(digest.size()), 0))) {
                constexpr wchar_t hex[] = L"0123456789abcdef";
                std::array<wchar_t, 65> actual{};
                for (size_t i = 0; i < digest.size(); ++i) {
                    actual[2*i] = hex[digest[i] >> 4];
                    actual[2*i+1] = hex[digest[i] & 15];
                }
                okay = wcscmp(actual.data(), kSha256) == 0;
            }
        }
    }
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    CloseHandle(file);
    return okay;
}

// Layout of the build's global Unreal object array header.
struct ObjectArray {
    std::uintptr_t chunks, preallocated;
    std::int32_t maximum, count, maxChunks, numChunks;
};
static_assert(sizeof(ObjectArray) == 32);

// Objects whose identities must remain stable while the temporary fix is active.
struct LocalPlayer {
    std::uintptr_t controller = 0, world = 0, info = 0;
};

// Require a playable authority world with no normal or replay network driver.
bool Standalone(std::uintptr_t world) {
    std::uintptr_t net = 0, demo = 0, mode = 0, level = 0;
    return Read(world+88, net) && !net && Read(world+304, demo) && !demo &&
           Read(world+464, mode) && mode && Read(world+80, level) && level;
}

// Before restoring, confirm the old info object still belongs to the same
// controller and has not joined a networked world.
bool SafeToRestore(const LocalPlayer& active) {
    std::uintptr_t net=0, demo=0, state=0, info=0;
    return active.world && active.controller && active.info &&
           Read(active.world+88, net) && !net &&
           Read(active.world+304, demo) && !demo &&
           Read(active.controller+912, state) && state &&
           Read(state+1496, info) && info == active.info;
}

// Verify the local player, controller, viewport and active world agree. This
// excludes AI controllers, stale worlds, lobby/transition worlds and network play.
bool Validate(std::uintptr_t controller, LocalPlayer& result) {
    std::uintptr_t player=0, backlink=0, viewport=0, level=0, world=0;
    std::uintptr_t persistent=0, viewportWorld=0, instance=0, viewportInstance=0;
    std::uintptr_t entries=0, state=0, info=0, connection=0;
    std::int32_t count=0;
    // These offsets follow Controller -> LocalPlayer -> Viewport -> World,
    // then World -> GameInstance -> LocalPlayers.
    if (!Read(controller+1608, player) || !player ||
        !Read(player+56, backlink) || backlink != controller ||
        !Read(player+120, viewport) || !viewport ||
        !Read(controller+40, level) || !level ||
        !Read(level+720, world) || !world ||
        !Read(world+80, persistent) || persistent != level ||
        !Read(viewport+128, viewportWorld) || viewportWorld != world ||
        !Read(world+560, instance) || !instance ||
        !Read(viewport+136, viewportInstance) || viewportInstance != instance ||
        !Read(instance+192, entries) || !entries ||
        !Read(instance+200, count) || count < 1 || count > 4 ||
        !Read(controller+1368, connection) || connection ||
        !Standalone(world)) return false;
    bool inLocalPlayers = false;
    for (std::int32_t i=0; i<count; ++i) {
        std::uintptr_t entry=0;
        if (!Read(entries+i*8, entry)) return false;
        inLocalPlayers |= entry == player;
    }
    if (!inLocalPlayers || !Read(controller+912, state) || !state ||
        !Read(state+1496, info) || !info) return false;
    result = {controller, world, info};
    return true;
}

// Load and validate the per-process byte substitution table for object pointers.
bool PointerTable(std::uintptr_t base, std::array<UCHAR,256>& table) {
    std::uintptr_t address=0;
    if (!Read(base+kTableRva, address) || !address ||
        !ReadBlock(address+0x100, table.data(), table.size())) return false;
    std::array<bool,256> seen{};
    for (UCHAR byte : table) {
        if (seen[byte]) return false;
        seen[byte] = true;
    }
    return true;
}

// Decode one obfuscated UObject pointer from a GObjects item.
std::uintptr_t Decode(const UCHAR* bytes, const std::array<UCHAR,256>& table) {
    std::uint64_t value=0;
    for (int i=0; i<8; ++i) value |= std::uint64_t(table[bytes[i]]) << (8*i);
    return static_cast<std::uintptr_t>(value ^ kPointerXor);
}

// Search the object array for the controller that passes every local-world
// relationship check. The scan runs only when no valid controller is cached.
LocalPlayer FindLocalPlayer(std::uintptr_t base) {
    ObjectArray objects{};
    if (!Read(base+kObjectsRva, objects) || objects.count <= 0 ||
        objects.count > objects.maximum || objects.maximum > 0x1000000 ||
        objects.numChunks <= 0 || objects.numChunks > objects.maxChunks ||
        objects.maxChunks >= 2048) return {};
    std::array<UCHAR,256> table{};
    if (!PointerTable(base, table)) return {};
    for (int c=0; c<objects.numChunks; ++c) {
        std::uintptr_t chunk=0;
        if (!Read(objects.chunks+c*8, chunk) || !chunk) break;
        const int remaining=objects.count-c*65536;
        if (remaining <= 0) break;
        const int n=remaining < 65536 ? remaining : 65536;
        std::vector<UCHAR> items(static_cast<size_t>(n)*40);
        if (!ReadBlock(chunk, items.data(), items.size())) break;
        for (int i=0; i<n; ++i) {
            std::uintptr_t object=Decode(items.data()+i*40+8, table);
            if (!object) continue;
            std::int32_t index=-1;
            if (!Read(object+12, index) || index != c*65536+i) continue;
            LocalPlayer candidate{};
            if (Validate(object, candidate)) return candidate;
        }
    }
    return {};
}

// Worker thread: verify the build, then apply and restore the local class
// eligibility level as worlds appear and disappear.
DWORD WINAPI Run(LPVOID) {
    AllocConsole();
    SetConsoleTitleW(L"SP Client Fixes");
    Log(L"SP Client Fixes DLL loaded. Checking game build...\r\n");
    if (!SupportedBuild()) {
        Log(L"Class selection fix disabled: unsupported executable SHA-256.\r\n");
        return 0;
    }
    Log(L"Supported build. Waiting for standalone local play...\r\n");
    const auto base=reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr));
    LocalPlayer active{};
    LONG original=-1;
    DWORD nextScan=0;
    for (;;) {
        LocalPlayer current{};
        if (active.controller && Validate(active.controller, current) &&
            current.world == active.world && current.info == active.info) {
            // The existing local world remains active.
        } else {
            if (original >= 0 && SafeToRestore(active) &&
                ChangeLevel(active.info+632, kClassLevel, original)) {
                Log(L"Class selection: original level restored.\r\n");
            }
            active={};
            original=-1;
            current={};
            if (static_cast<LONG>(GetTickCount()-nextScan) >= 0) {
                current=FindLocalPlayer(base);
                nextScan=GetTickCount()+2000;
            }
        }
        if (current.info) {
            LONG level=-1;
            if (Read(current.info+632, level) && level >= 0 && level < kClassLevel &&
                ChangeLevel(current.info+632, level, kClassLevel)) {
                active=current;
                original=level;
                Log(L"Class selection: local level set to 5; class tiles are available.\r\n");
            }
        }
        Sleep(250);
    }
}
} // namespace

// Launcher-facing version marker for the optional client fixes module.
extern "C" __declspec(dllexport) unsigned int SPClientFixesVersion() { return 2; }

// Leave the loader callback quickly; the worker does all hashing and game reads.
BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(module);
        HANDLE thread=CreateThread(nullptr, 0, Run, nullptr, 0, nullptr);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
