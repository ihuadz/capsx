; CapsX M1 安装脚本（Inno Setup 6）
; 单用户安装，不需要管理员权限。
; 编译：iscc installer\capsx.iss

#define MyAppName "CapsX"
; 版本号默认取这里的值；CI 打 tag 时用 iscc /DMyAppVersion=x.y.z 覆盖。
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#ifndef MyAppVersionQuad
  #define MyAppVersionQuad "0.1.0.0"
#endif
#define MyAppPublisher "CapsX Contributors"
#define MyAppURL "https://github.com/ihuadz/capsx"
#define MyAppExeName "capsx.exe"

[Setup]
AppId={{A71322A6-91C2-4949-9DCC-590D31D084EC}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={localappdata}\Programs\CapsX
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=CapsX-Setup-{#MyAppVersion}
SetupIconFile=..\assets\logo\capsx.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
UsePreviousAppDir=yes
AllowNoIcons=yes
SetupLogging=yes
VersionInfoVersion={#MyAppVersionQuad}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："; Flags: unchecked

[Files]
Source: "..\target\release\capsx.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\runtime\kanata.exe"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\runtime\LICENSE.kanata.txt"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "..\runtime\default.kbd"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\logo\capsx-wordmark.png"; DestDir: "{app}\assets\logo"; Flags: ignoreversion
Source: "..\docs\ACCEPTANCE.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "..\docs\screenshots\settings.png"; DestDir: "{app}\docs\screenshots"; Flags: ignoreversion
Source: "..\docs\screenshots\settings-mappings.png"; DestDir: "{app}\docs\screenshots"; Flags: ignoreversion
Source: "..\crates\capsx-app\assets\fonts\OFL.txt"; DestDir: "{app}\licenses"; DestName: "OFL-NotoSansSC.txt"; Flags: ignoreversion
Source: "..\assets\logo\capsx.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--tray"; Description: "启动 CapsX"; Flags: nowait postinstall skipifsilent

[Registry]
; 兜底：即使退出事件失败，卸载也要删除开机自启项。
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "CapsX"; Flags: dontcreatekey uninsdeletevalue

[Code]
const
  MutexName = 'CapsX.SingleInstance';

function StopRunningCapsX(): Boolean;
var
  ResultCode: Integer;
  I: Integer;
begin
  Result := True;
  if not FileExists(ExpandConstant('{app}\capsx.exe')) then
    exit;

  { 先走应用的正常退出路径：断开 TCP、结束 kanata、保存设置。 }
  Exec(
    ExpandConstant('{app}\capsx.exe'),
    '--uninstall',
    '',
    SW_HIDE,
    ewNoWait,
    ResultCode
  );

  { 等待单实例互斥体释放，最多 10 秒。 }
  for I := 1 to 50 do
  begin
    if not CheckForMutexes(MutexName) then
      exit;
    Sleep(200);
  end;

  { 超时兜底强杀；Job Object 会同时回收 kanata。 }
  Exec('taskkill.exe', '/F /IM capsx.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
  Result := not CheckForMutexes(MutexName);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  NeedsRestart := False;
  StopRunningCapsX();
end;

function InitializeUninstall(): Boolean;
begin
  StopRunningCapsX();
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    if SuppressibleMsgBox(
      '是否保留 CapsX 的设置与键位？' + #13#10 + #13#10 +
      '选择“是”：保留 %APPDATA%\CapsX，下次安装继续使用。' + #13#10 +
      '选择“否”：删除设置和日志。',
      mbConfirmation,
      MB_YESNO,
      IDYES
    ) = IDNO then
    begin
      DelTree(ExpandConstant('{userappdata}\CapsX'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\CapsX'), True, True, True);
    end;
  end;
end;
