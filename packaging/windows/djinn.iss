; DJINN installer for Windows (Inno Setup 6).
;
;   iscc packaging\windows\djinn.iss
;   iscc /DVersion=0.2.0 /DBinary=C:\path\djinn.exe packaging\windows\djinn.iss
;
; Build the binary first (see CLAUDE.md). Installs for the current user
; only, under %LOCALAPPDATA%\Programs\DJINN, so no admin rights are needed
; and DJINN can update itself in place. On Windows DJINN keeps its config,
; database, log, art and AI models next to djinn.exe; uninstalling keeps
; them unless the user asks to remove them.

#ifndef Binary
  #define Binary "..\..\target\release\djinn.exe"
#endif
#ifndef Version
  ; A local build uses Cargo's version. Release builds pass the padded date.
  #define Version GetStringFileInfo(Binary, "ProductVersion")
#endif

[Setup]
AppId={{0DDB8519-B7C2-4BF6-90FC-D6F29123ED90}
AppName=DJINN
AppVersion={#Version}
AppVerName=DJINN {#Version}
AppPublisher=New Terra Studios
AppPublisherURL=https://github.com/jmarasch/DJINN
AppSupportURL=https://github.com/jmarasch/DJINN/issues
AppUpdatesURL=https://github.com/jmarasch/DJINN/releases
AppContact=DJINN@newterrastudios.com
DefaultDirName={localappdata}\Programs\DJINN
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE.md
SetupIconFile=djinn.ico
UninstallDisplayIcon={app}\djinn.exe
UninstallDisplayName=DJINN
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; PrepareToInstall asks a running DJINN to quit first; this only catches
; what that missed.
CloseApplications=force
RestartApplications=no
OutputDir=..\..\target\installer
OutputBaseFilename=djinn-setup-windows-x86_64

[Tasks]
Name: desktopicon; Description: "Put a DJINN shortcut on the desktop"; Flags: unchecked
Name: autostart; Description: "Start DJINN when I sign in to Windows"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "djinn.exe"; Flags: ignoreversion
Source: "djinn.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\DJINN"; Filename: "{app}\djinn.exe"; IconFilename: "{app}\djinn.ico"; Comment: "Party jukebox, lives in the system tray"
Name: "{autodesktop}\DJINN"; Filename: "{app}\djinn.exe"; IconFilename: "{app}\djinn.ico"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "DJINN"; ValueData: """{app}\djinn.exe"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\djinn.exe"; Description: "Start DJINN now"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Left behind by an in-app update.
Type: files; Name: "{app}\djinn.old.exe"
Type: dirifempty; Name: "{app}"

[Code]
function DjinnRunning(): Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C tasklist /NH /FI "IMAGENAME eq djinn.exe" | find /I "djinn.exe"',
    '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
end;

// Asks a running DJINN to quit (taskkill without /F sends WM_CLOSE, which
// DJINN treats like Shut down in the tray), waits for it to save and exit,
// and only forces it if it doesn't.
procedure StopDjinn();
var
  Code, Waited: Integer;
begin
  if not DjinnRunning() then
    Exit;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM djinn.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Waited := 0;
  while DjinnRunning() and (Waited < 15000) do
  begin
    Sleep(250);
    Waited := Waited + 250;
  end;
  if DjinnRunning() then
    Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM djinn.exe /IM djinn-gpu.exe', '', SW_HIDE,
      ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopDjinn();
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep = usUninstall then
    StopDjinn();
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
  begin
    // Whatever is left in the folder is DJINN's own data.
    Data := ExpandConstant('{app}');
    if DirExists(Data) and (MsgBox('Also remove your DJINN settings, users, playlists, play history and downloaded AI models?' + #13#10#13#10 +
        'Your music is never touched either way.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(Data, True, True, True);
  end;
end;
