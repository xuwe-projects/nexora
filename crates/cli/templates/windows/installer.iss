[Setup]
AppId={#AppId}
AppName={#AppName}
AppVerName={#AppName} {#AppVersion}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
VersionInfoVersion={#FileVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} 安装程序
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#FileVersion}
DefaultDirName={localappdata}\Programs\{#AppId}
DisableDirPage=no
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SourceDir={#SourceDir}
SetupIconFile={#IconPath}
UninstallDisplayIcon={app}\{#MainExeName}
UninstallDisplayName={#AppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableWelcomePage=no
ShowLanguageDialog=no
MinVersion=10.0.{#MinimumWindowsBuild}
ArchitecturesAllowed={#ArchitectureAllowed}
ArchitecturesInstallIn64BitMode={#ArchitectureInstallMode}
CloseApplications=force
CloseApplicationsFilter={#MainExeName}
RestartApplications=no
SetupMutex={#AppId}.setup

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\Unofficial\ChineseSimplified.isl"

[Tasks]
#if DesktopShortcutDefault
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
#else
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
#endif
#if StartMenuShortcutDefault
Name: "startmenuicon"; Description: "创建开始菜单快捷方式"; GroupDescription: "{cm:AdditionalIcons}"
#else
Name: "startmenuicon"; Description: "创建开始菜单快捷方式"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
#endif

[Files]
Source: "*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#MainExeName}"; WorkingDir: "{app}"; Tasks: desktopicon; AppUserModelID: "{#AppId}"
Name: "{group}\{#AppName}"; Filename: "{app}\{#MainExeName}"; WorkingDir: "{app}"; Tasks: startmenuicon; AppUserModelID: "{#AppId}"

[Run]
#if LaunchAfterInstallDefault
Filename: "{app}\{#MainExeName}"; Description: "安装完成后运行 {#AppName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
#else
Filename: "{app}\{#MainExeName}"; Description: "安装完成后运行 {#AppName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent unchecked
#endif
