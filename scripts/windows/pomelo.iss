#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef BuildDir
  #error BuildDir is required
#endif
#ifndef IconFile
  #error IconFile is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef AppArch
  #define AppArch "x64"
#endif
#if AppArch == "arm64"
  #define AllowedArchitectures "arm64"
#elif AppArch == "x64"
  #define AllowedArchitectures "x64compatible"
#else
  #error AppArch must be x64 or arm64
#endif

[Setup]
AppId={{F60DEFB7-4A95-4EB2-B561-1F776C0993A5}
AppName=Pomelo
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Pomelo
DefaultGroupName=Pomelo
PrivilegesRequired=lowest
ArchitecturesAllowed={#AllowedArchitectures}
ArchitecturesInstallIn64BitMode={#AllowedArchitectures}
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Pomelo-{#AppVersion}-windows-{#AppArch}-setup
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\pomelo.exe
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#BuildDir}\pomelo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\fonts\source-han-sans\LICENSE.txt"; DestDir: "{app}\licenses"; DestName: "Source-Han-Sans-OFL.txt"; Flags: ignoreversion
Source: "..\..\crates\pomelo-core\src\search\data\LICENSE"; DestDir: "{app}\licenses"; DestName: "Unicode-License.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pomelo"; Filename: "{app}\pomelo.exe"; IconFilename: "{app}\pomelo.exe"
Name: "{autodesktop}\Pomelo"; Filename: "{app}\pomelo.exe"; IconFilename: "{app}\pomelo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\pomelo.exe"; Description: "Launch Pomelo"; Flags: nowait postinstall skipifsilent
