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
DefaultDirName={localappdata}\Programs\{#AppPublisher}\{#AppName}
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
RestartApplications=no
SetupMutex={#AppId}.setup

[Languages]
Name: "chinesesimplified"; MessagesFile: "{#LanguageFile}"

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

[Code]
function NormalizedPath(const Value: String): String;
begin
  Result := AddBackslash(ExpandFileName(Value));
end;

function DirectoryIsEmpty(const Directory: String): Boolean;
var
  FindRec: TFindRec;
begin
  Result := True;
  if not DirExists(Directory) then
    Exit;

  if FindFirst(AddBackslash(Directory) + '*', FindRec) then
  begin
    try
      repeat
        if (FindRec.Name <> '.') and (FindRec.Name <> '..') then
        begin
          Result := False;
          Exit;
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

function ExistingStableInstallMatches(const Directory: String): Boolean;
var
  ExistingPath: String;
begin
  Result := False;
#if StableChannel
  if RegQueryStringValue(
    HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppId}_is1',
    'InstallLocation',
    ExistingPath) then
    Result := NormalizedPath(ExistingPath) = NormalizedPath(Directory);
#endif
end;

function InstallDirectoryHasExpectedIdentity(const Directory: String): Boolean;
var
  MarkerPath: String;
  MarkerValue: AnsiString;
begin
  MarkerPath := AddBackslash(Directory) + 'nexora-install-identity';
  if FileExists(MarkerPath) then
  begin
    if not LoadStringFromFile(MarkerPath, MarkerValue) then
    begin
      Result := False;
      Exit;
    end;
    Result := Trim(String(MarkerValue)) = '{#InstallIdentity}';
    Exit;
  end;

  Result := DirectoryIsEmpty(Directory) or ExistingStableInstallMatches(Directory);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpSelectDir) and
     not InstallDirectoryHasExpectedIdentity(WizardDirValue) then
  begin
    MsgBox(
      '所选目录属于其他发布通道，或不是可识别的 {#AppName} 安装目录。' + #13#10 +
      '请选择空目录，或选择当前通道已经安装的目录。',
      mbError,
      MB_OK);
    Result := False;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not InstallDirectoryHasExpectedIdentity(WizardDirValue) then
    Result :=
      '所选目录属于其他发布通道，或不是可识别的 {#AppName} 安装目录。' + #13#10 +
      '请选择空目录，或选择当前通道已经安装的目录。';
end;
