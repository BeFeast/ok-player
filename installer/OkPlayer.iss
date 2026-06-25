; OK Player — Inno Setup script. Packages the self-contained, unpackaged WinUI 3 build into a
; friendly installer (.exe) with Start-menu / optional desktop shortcuts and an uninstaller.
;
; Don't run this by hand — use installer\build-installer.ps1, which publishes the app, stages the
; licence/notices and invokes ISCC with the right defines. To build directly:
;   ISCC.exe /DSourceDir=<publish folder> /DAppVersion=0.2.0 /DRepoRoot=<repo root> OkPlayer.iss
; SourceDir must be a self-contained `dotnet publish -c Release` output (exe + OkPlayer.pri +
; libmpv-2.dll + the bundled .NET / Windows App SDK runtime + LICENSE.txt + THIRD-PARTY-NOTICES.md).

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\artifacts\publish"
#endif
#ifndef RepoRoot
  #define RepoRoot ".."
#endif

#define AppName "OK Player"
#define AppPublisher "BeFeast"
#define AppURL "https://github.com/BeFeast/ok-player"
#define AppExe "OkPlayer.exe"

[Setup]
AppId={{B18A762D-0D36-42AD-9A41-C4B704ADDE90}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
DefaultDirName={autopf}\OK Player
DefaultGroupName=OK Player
DisableProgramGroupPage=yes
DisableDirPage=auto
LicenseFile={#SourceDir}\LICENSE.txt
OutputBaseFilename=OkPlayer-Setup-v{#AppVersion}-win-x64
SetupIconFile={#RepoRoot}\src\OkPlayer.App\Assets\OkPlayer.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22621
; per-user install by default (no admin); the user can elevate to all-users in the wizard
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\OK Player"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,OK Player}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\OK Player"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,OK Player}"; Flags: nowait postinstall skipifsilent
