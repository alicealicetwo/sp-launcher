# Client fixes DLL

`SPClientFixes.dll` is a separate, optional 64-bit DLL. The launcher installs
it beside the game executable and loads it only when **Settings → Client
fixes** is enabled. The existing `XAPOFX1_5.dll` no-Steam proxy is unchanged.

This is currently a loadable shell with **no gameplay hooks**. Add the
preservation fixes here after translating and verifying each build-specific
finding. The DLL must verify the shipping executable's SHA-256
`16b8b421371457d936e5cc1810ff707b5f5984126973dbdc4e6b5c714522051f`
before applying any fix. Local-only fixes must also check the active world's
standalone state at the point they run; enabling the DLL is not permission to
change online matches.

Build with a 64-bit Visual Studio toolchain:

```powershell
cmake -S client-fixes -B client-fixes/build -A x64
cmake --build client-fixes/build --config Release
Copy-Item client-fixes/build/Release/SPClientFixes.dll src-tauri/resources/SPClientFixes.dll
```

Build the launcher after copying the DLL. A fresh checkout can compile without
the binary, but enabling Client fixes then refuses to start the game with a
clear error. The release script also requires the DLL so a published toggle is
usable.
