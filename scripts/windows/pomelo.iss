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

[Setup]
AppId={{F60DEFB7-4A95-4EB2-B561-1F776C0993A5}
AppName=Pomelo
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Pomelo
DefaultGroupName=Pomelo
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Pomelo-{#AppVersion}-windows-x64-setup
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
Source: "..\..\assets\licenses\fdlibm-v8.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pomelo"; Filename: "{app}\pomelo.exe"; IconFilename: "{app}\pomelo.exe"
Name: "{autodesktop}\Pomelo"; Filename: "{app}\pomelo.exe"; IconFilename: "{app}\pomelo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\pomelo.exe"; Description: "Launch Pomelo"; Flags: nowait postinstall skipifsilent
