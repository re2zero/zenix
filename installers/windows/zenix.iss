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
; Main binary
Source: "target\release\zenix.exe"; DestDir: "{app}"; Flags: ignoreversion

; Companion binary (built by build.rs, may not exist)
Source: "target\release\herdr.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

; Desktop entry (will be installed alongside binary for reference)
Source: "res\zenix.desktop"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

; Icon
Source: "res\zenix.png"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

; License texts — same inventory as [package.metadata.deb] in Cargo.toml. The
; Lilex faces are compiled into the binary, so no font files ship, but OFL-1.1
; still requires the license text to travel with the font. Windows has no
; /usr/share/doc, so they go under {app}\doc: Inno tracks them as installed
; files and removes them on uninstall, which {commonappdata} would not (it is
; machine-wide shared state the uninstaller leaves behind).
Source: "LICENSE"; DestDir: "{app}\doc"; Flags: ignoreversion
Source: "THIRD-PARTY.md"; DestDir: "{app}\doc"; Flags: ignoreversion
Source: "assets\fonts\OFL.txt"; DestDir: "{app}\doc"; DestFileName: "Lilex-OFL.txt"; Flags: ignoreversion
Source: "herdr\LICENSE"; DestDir: "{app}\doc\herdr"; Flags: ignoreversion
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
