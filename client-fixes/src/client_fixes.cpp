#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <cstdint>
#include <cwchar>
#include <string>
#include <unordered_map>
#include <vector>

namespace {
// BravoHotel 1.3.0.473797 only. Never use these offsets on another binary.
constexpr wchar_t kSha256[] = L"16b8b421371457d936e5cc1810ff707b5f5984126973dbdc4e6b5c714522051f";
constexpr std::uintptr_t kObjectsRva = 0x762f708;
constexpr std::uintptr_t kTableRva = 0x6b3c0f8;
constexpr std::uint64_t kPointerXor = 0xba146ab1e38e3211;
constexpr LONG kClassLevel = 5;
constexpr std::uintptr_t kNamePoolRva = 0x7603240;
constexpr std::uintptr_t kAnsiNameDecoderRva = 0x2a5dfb0;
constexpr std::uintptr_t kWideNameDecoderRva = 0x2a6a710;

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
// Change an aligned 32-bit field only if it still has the expected value.
// Catch access violations if the game destroys the object between checks.
bool CompareDword(std::uintptr_t address, LONG before, LONG after) {
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

// The game's FName pool stores encrypted text. These two exact-build native
// routines are the same decoders used by the read-only reflection inventory.
bool CallNameDecoder(std::uintptr_t function, std::uintptr_t entry,
                     void* output, std::size_t length) {
    using Decoder = void(__fastcall*)(const void*, void*, std::size_t);
    __try {
        reinterpret_cast<Decoder>(function)(reinterpret_cast<const void*>(entry),
                                            output, length);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// Resolve an FName index to its base text; the instance number is separate.
bool Name(std::uintptr_t base, std::uint32_t index, std::string& result) {
    std::uintptr_t block=0;
    if (!Read(base+kNamePoolRva+16+8*(index>>16), block) || !block) return false;
    const auto entry=block+2*(index&0xffff);
    std::uint16_t header=0;
    if (!Read(entry, header)) return false;
    const std::size_t length=header>>6;
    if (!length || length>512) return false;
    std::array<UCHAR, 2048> output{};
    const bool wide=(header&1)!=0;
    if (!CallNameDecoder(base+(wide?kWideNameDecoderRva:kAnsiNameDecoderRva),
                         entry, output.data(), length)) return false;
    result.clear();
    result.reserve(length);
    if (wide) {
        for (std::size_t i=0; i<length; ++i) {
            const auto letter=std::uint16_t(output[2*i]) |
                              (std::uint16_t(output[2*i+1])<<8);
            if (!letter || letter>0x7f) return false;
            result.push_back(static_cast<char>(letter));
        }
    } else {
        for (std::size_t i=0; i<length; ++i) {
            if (!output[i] || output[i]>0x7f) return false;
            result.push_back(static_cast<char>(output[i]));
        }
    }
    return true;
}

// Read the base FName of a UObject, such as DataTable or TBL-BuffData.
bool ObjectNamed(std::uintptr_t base, std::uintptr_t object, const char* wanted) {
    std::uint32_t index=0;
    std::string name;
    return Read(object+16, index) && Name(base, index, name) && name==wanted;
}

struct RowMap {
    std::uintptr_t entries=0;
    std::int32_t count=0, capacity=0;
};
// Read a DataTable's row map header and reject implausible sizes.
bool ReadRowMap(std::uintptr_t table, RowMap& map) {
    return Read(table+0x38, map) && map.entries && map.count>0 &&
           map.count<=map.capacity && map.capacity<=20000;
}

struct NamedRows {
    std::uintptr_t first=0, second=0;
    std::uint32_t firstKey=0, secondKey=0;
    std::int32_t firstPosition=-1, secondPosition=-1;
    std::uintptr_t mapEntries=0;
};

// Find two named rows in a UE DataTable's 32-byte TMap entries. Verify the
// row pointers are readable; empty or stale map slots are ignored.
NamedRows FindNamedRows(std::uintptr_t base, std::uintptr_t table,
                        const char* firstName, const char* secondName) {
    RowMap map{};
    NamedRows rows{};
    if (!ReadRowMap(table, map)) return rows;
    rows.mapEntries=map.entries;
    for (int i=0; i<map.capacity && (!rows.first || !rows.second); ++i) {
        const auto slot=map.entries+static_cast<std::uintptr_t>(i)*32;
        std::uint32_t key=0;
        std::uintptr_t row=0;
        if (!Read(slot, key) || !Read(slot+16, row) || !row) continue;
        std::string name;
        if (!Name(base, key, name)) continue;
        std::uint64_t rowStart=0;
        if (!Read(row, rowStart)) continue;
        if (name==firstName) { rows.first=row; rows.firstKey=key; rows.firstPosition=i; }
        if (name==secondName) { rows.second=row; rows.secondKey=key; rows.secondPosition=i; }
    }
    return rows;
}

// Compare a UE FString field with an expected short UTF-16 value.
bool StringEquals(std::uintptr_t address, const wchar_t* expected) {
    std::uintptr_t data=0;
    std::int32_t count=0, capacity=0;
    if (!Read(address, data) || !Read(address+8, count) ||
        !Read(address+12, capacity) || !data ||
        count<1 || count>capacity || capacity>64) return false;
    const auto length=wcslen(expected);
    if (count!=static_cast<std::int32_t>(length) &&
        count!=static_cast<std::int32_t>(length+1)) return false;
    std::array<wchar_t,64> contents{};
    return ReadBlock(data, contents.data(), count*sizeof(wchar_t)) &&
           wcscmp(contents.data(), expected)==0;
}

struct CapsuleRows {
    std::uintptr_t table=0, white=0, gold=0;
    std::uintptr_t mapEntries=0;
    std::int32_t whitePosition=-1, goldPosition=-1;
    std::uint32_t whiteKey=0, goldKey=0;
    std::uint32_t whiteBuff=0, goldBuff=0;
};

// Locate the merged item table and the authoritative buff table by object,
// row-struct and row names. No session-specific addresses are retained.
CapsuleRows FindCapsuleRows(std::uintptr_t base) {
    CapsuleRows result{};
    std::string zero;
    if (!Name(base, 0, zero) || zero!="None") return result;
    ObjectArray objects{};
    std::array<UCHAR,256> pointerTable{};
    if (!Read(base+kObjectsRva, objects) || objects.count<=0 ||
        objects.count>objects.maximum || objects.maximum>0x1000000 ||
        objects.numChunks<=0 || objects.numChunks>objects.maxChunks ||
        objects.maxChunks>=2048 || !PointerTable(base, pointerTable)) return result;
    NamedRows items{}, buffs{};
    std::uintptr_t itemTable=0;
    std::unordered_map<std::uintptr_t,bool> dataTableClass;
    for (int c=0; c<objects.numChunks; ++c) {
        std::uintptr_t chunk=0;
        if (!Read(objects.chunks+c*8, chunk) || !chunk) break;
        const int remaining=objects.count-c*65536;
        if (remaining<=0) break;
        const int n=remaining<65536?remaining:65536;
        std::vector<UCHAR> slots(static_cast<std::size_t>(n)*40);
        if (!ReadBlock(chunk, slots.data(), slots.size())) break;
        for (int i=0; i<n; ++i) {
            const auto object=Decode(slots.data()+i*40+8, pointerTable);
            if (!object) continue;
            std::int32_t index=-1;
            std::uintptr_t cls=0;
            if (!Read(object+12, index) || index!=c*65536+i ||
                !Read(object+32, cls) || !cls) continue;
            auto known=dataTableClass.find(cls);
            if (known==dataTableClass.end())
                known=dataTableClass.emplace(cls, ObjectNamed(base,cls,"DataTable") ||
                                                     ObjectNamed(base,cls,"CompositeDataTable")).first;
            if (!known->second) continue;
            std::uintptr_t rowStruct=0;
            if (!Read(object+48, rowStruct) || !rowStruct) continue;
            if (!itemTable && ObjectNamed(base,object,"DataTable") &&
                ObjectNamed(base,rowStruct,"InventoryItemDetailInfo")) {
                auto found=FindNamedRows(base,object,"Tablet_White","Tablet_Black");
                if (found.first && found.second) { itemTable=object; items=found; }
            }
            if (!buffs.first && ObjectNamed(base,object,"TBL-BuffData") &&
                ObjectNamed(base,rowStruct,"BuffData")) {
                auto found=FindNamedRows(base,object,"220000104","220000105");
                // These are the All-skills +2 and +3 buffs verified in the
                // live memory experiment.
                if (found.first && found.second &&
                    StringEquals(found.first+376,L"All") &&
                    StringEquals(found.first+392,L"2") &&
                    StringEquals(found.second+376,L"All") &&
                    StringEquals(found.second+392,L"3")) buffs=found;
            }
            if (itemTable && buffs.first) break;
        }
        if (itemTable && buffs.first) break;
    }
    if (!itemTable || !buffs.first) return {};
    result={itemTable,items.first,items.second,items.mapEntries,
            items.firstPosition,items.secondPosition,items.firstKey,items.secondKey,
            buffs.firstKey,buffs.secondKey};
    return result;
}

// Confirm the tracked rows are still entries in the same merged table.
bool RowsStillMapped(const CapsuleRows& rows) {
    RowMap map{};
    if (!rows.table || !ReadRowMap(rows.table,map) ||
        map.entries!=rows.mapEntries ||
        rows.whitePosition<0 || rows.whitePosition>=map.capacity ||
        rows.goldPosition<0 || rows.goldPosition>=map.capacity) return false;
    auto matches=[&map](std::int32_t position, std::uint32_t expectedKey,
                        std::uintptr_t expectedRow) {
        const auto slot=map.entries+static_cast<std::uintptr_t>(position)*32;
        std::uint32_t key=0;
        std::uintptr_t row=0;
        return Read(slot,key) && Read(slot+16,row) &&
               key==expectedKey && row==expectedRow;
    };
    return matches(rows.whitePosition,rows.whiteKey,rows.white) &&
           matches(rows.goldPosition,rows.goldKey,rows.gold);
}

// The first FName in each UsingBuffName array is the four-byte index changed
// by the successful live experiment. Verify both original IDs before writing.
bool CapsuleSlots(std::uintptr_t base, const CapsuleRows& rows,
                  std::uintptr_t& whiteSlot, std::uintptr_t& goldSlot) {
    auto slot=[base](std::uintptr_t row, const char* broken,
                     const char* fixed, std::uintptr_t& address) {
        std::uintptr_t array=0;
        std::int32_t count=0, capacity=0;
        if (!Read(row+0x508,array) || !array ||
            !Read(row+0x510,count) || !Read(row+0x514,capacity) ||
            count!=1 || capacity<count || capacity>16) return false;
        std::uint32_t index=0;
        std::string name;
        if (!Read(array,index) || !Name(base,index,name) ||
            (name!=broken && name!=fixed)) return false;
        address=array;
        return true;
    };
    return slot(rows.white,"221000337","220000104",whiteSlot) &&
           slot(rows.gold,"221000338","220000105",goldSlot);
}

struct CapsulePatch {
    CapsuleRows rows{};
    std::uintptr_t whiteSlot=0, goldSlot=0;
    std::uint32_t whiteOriginal=0, goldOriginal=0;
};

// Apply both remaps as one unit. If the second write fails, undo the first.
bool ApplyCapsulePatch(std::uintptr_t base, const CapsuleRows& rows,
                       CapsulePatch& patch) {
    std::uintptr_t whiteSlot=0, goldSlot=0;
    if (!rows.whiteBuff || !rows.goldBuff || !RowsStillMapped(rows) ||
        !CapsuleSlots(base,rows,whiteSlot,goldSlot)) return false;
    std::uint32_t white=0,gold=0;
    if (!Read(whiteSlot,white) || !Read(goldSlot,gold)) return false;
    std::string whiteName,goldName;
    if (!Name(base,white,whiteName) || whiteName!="221000337" ||
        !Name(base,gold,goldName) || goldName!="221000338") return false;
    if (!CompareDword(whiteSlot,static_cast<LONG>(white),
                     static_cast<LONG>(rows.whiteBuff))) return false;
    if (!CompareDword(goldSlot,static_cast<LONG>(gold),
                     static_cast<LONG>(rows.goldBuff))) {
        CompareDword(whiteSlot,static_cast<LONG>(rows.whiteBuff),static_cast<LONG>(white));
        return false;
    }
    patch={rows,whiteSlot,goldSlot,white,gold};
    return true;
}

// Keep the corrected merged rows present for the life of this game process.
// The merged table can be loaded or rebuilt after the DLL has started.
DWORD WINAPI RunCapsules(LPVOID imageBase) {
    const auto base=reinterpret_cast<std::uintptr_t>(imageBase);
    CapsulePatch patch{};
    for (;;) {
        if (patch.rows.table) {
            if (!RowsStillMapped(patch.rows)) {
                patch={};
                Log(L"Capsule fix: merged item table changed; searching again.\r\n");
            } else {
                std::uint32_t white=0, gold=0;
                if (!Read(patch.whiteSlot,white) || !Read(patch.goldSlot,gold) ||
                    (white!=patch.whiteOriginal && white!=patch.rows.whiteBuff) ||
                    (gold!=patch.goldOriginal && gold!=patch.rows.goldBuff)) {
                    patch={};
                } else {
                    // Reapply if the game rebuilt the row values in place.
                    if (white==patch.whiteOriginal)
                        CompareDword(patch.whiteSlot,static_cast<LONG>(white),
                                    static_cast<LONG>(patch.rows.whiteBuff));
                    if (gold==patch.goldOriginal)
                        CompareDword(patch.goldSlot,static_cast<LONG>(gold),
                                    static_cast<LONG>(patch.rows.goldBuff));
                }
            }
        }
        if (!patch.rows.table) {
            const CapsuleRows rows=FindCapsuleRows(base);
            if (rows.table && ApplyCapsulePatch(base,rows,patch))
                Log(L"Capsule fix: White and Gold buff IDs corrected.\r\n");
        }
        Sleep(patch.rows.table ? 1000 : 5000);
    }
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
    Log(L"Supported build. Starting client fixes...\r\n");
    const auto base=reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr));
    HANDLE capsuleThread=CreateThread(nullptr,0,RunCapsules,
                                      reinterpret_cast<LPVOID>(base),0,nullptr);
    if (capsuleThread) CloseHandle(capsuleThread);
    else Log(L"Capsule fix: could not start worker thread.\r\n");
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
                CompareDword(active.info+632, kClassLevel, original)) {
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
                CompareDword(current.info+632, level, kClassLevel)) {
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
extern "C" __declspec(dllexport) unsigned int SPClientFixesVersion() { return 3; }

// Leave the loader callback quickly; the worker does all hashing and game reads.
BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(module);
        HANDLE thread=CreateThread(nullptr, 0, Run, nullptr, 0, nullptr);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
