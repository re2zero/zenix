; Inno Setup script for zenix Windows installer.
;
; Build in CI:
;   iscc "/DMyAppVersion=1.0.0" zenix.iss
;
; Or install Inno Setup locally and right-click → Compile.

#define MyAppName "zenix"
#define MyAppPublisher "re2zero"
#define MyAppURL "https://github.com/re2zero/zenix"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

[Setup]
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
DefaultDirName={autopf64}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=.
OutputBaseFilename=zenix-{#MyAppVersion}-x86_64-setup
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\zenix.exe
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
SourceDir=..\..

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinese"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
; Asset inventory mirrors [package.metadata.deb] in Cargo.toml, which is the
; authoritative list; the only omissions are the ones the platform forces and
; they are called out below. Paths cannot match: the .deb spreads files over
; /usr/{bin,share/...}, the Windows tree is flat under {app}.
;
; Main binary
Source: "target\release\zenix.exe"; DestDir: "{app}"; Flags: ignoreversion

; No companion binary. build.rs never builds herdr here — build_herdr is Unix
; only, herdr being a Zig build — so target\release\herdr.exe can only appear
; through its Windows fallback, which copies whatever herdr.exe was on the
; builder's PATH. Shipping that would pin an uncontrolled daemon version, the
; failure the release-profile build exists to prevent, and the old
; `skipifsourcedoesntexist` entry made the installer's contents depend on
; whether the CI machine happened to have herdr. So: no seed, deliberately.
; The client still checks {app}\herdr.exe (src/platform.rs::seed_herdr, i.e.
; the default C:\Program Files\zenix) and then PATH (src/client/mod.rs), so a
; user who installs herdr themselves is unaffected. If a Windows herdr ever
; becomes buildable, add it here as an ordinary {app} file — never conditional.

; Desktop entry — nothing on Windows reads it; installed as a reference copy so
; the tree carries the same desktop entry and icon the .deb ships.
Source: "res\zenix.desktop"; DestDir: "{app}"; Flags: ignoreversion

; Icon
Source: "res\zenix.png"; DestDir: "{app}"; Flags: ignoreversion

; License texts — same inventory as [package.metadata.deb] in Cargo.toml, the
; one rename being the project license ({app}\doc\LICENSE here,
; usr/share/doc/zenix/copyright in the .deb because cargo-deb uses that name).
; The Lilex faces are compiled into the binary, so no font files ship, but
; OFL-1.1 still requires the license text to travel with the font. Windows has
; no /usr/share/doc, so they go under {app}\doc: Inno tracks them as installed
; files and removes them on uninstall, which {commonappdata} would not (it is
; machine-wide shared state the uninstaller leaves behind).
Source: "LICENSE"; DestDir: "{app}\doc"; Flags: ignoreversion
Source: "THIRD-PARTY.md"; DestDir: "{app}\doc"; Flags: ignoreversion
Source: "assets\fonts\OFL.txt"; DestDir: "{app}\doc"; DestFileName: "Lilex-OFL.txt"; Flags: ignoreversion
Source: "licenses\herdr\LICENSE"; DestDir: "{app}\doc\herdr"; Flags: ignoreversion
Source: "vendor\herdr-client\LICENSE-APACHE"; DestDir: "{app}\doc\herdr-client"; Flags: ignoreversion
Source: "vendor\herdr-client\NOTICE.md"; DestDir: "{app}\doc\herdr-client"; Flags: ignoreversion
Source: "vendor\herdr-protocol\LICENSE-APACHE"; DestDir: "{app}\doc\herdr-protocol"; Flags: ignoreversion
Source: "vendor\herdr-protocol\NOTICE.md"; DestDir: "{app}\doc\herdr-protocol"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\zenix.exe"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{commondesktop}\{#MyAppName}"; Filename: "{app}\zenix.exe"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Run]
Filename: "{app}\zenix.exe"; Description: "Launch {#MyAppName}"; Flags: postinstall nowait skipifsilent unchecked

[UninstallRun]
Filename: "{cmd}"; Parameters: "/c taskkill /f /im zenix.exe 2>nul"; Flags: runhidden
