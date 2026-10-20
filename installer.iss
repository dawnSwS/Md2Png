#define MyAppName "Md2Png"
#define MyAppExeName "md2png.exe"
#ifndef MyAppVersion
  #define MyAppVersion "2.0.0"
#endif

[Setup]
AppId={{31ef64cc-e01b-437b-92a8-f153afb8b817}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
OutputDir=Output
OutputBaseFilename=Md2Png_Setup
Compression=lzma2/max
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
#if FileExists("app_icon.ico")
SetupIconFile=app_icon.ico
#endif

[Files]
Source: "target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Registry]
Root: HKCR; Subkey: "Directory\Background\shell\{#MyAppName}"; ValueType: string; ValueData: "粘贴 Markdown 为图片 (原生引擎)"; Flags: uninsdeletekey
Root: HKCR; Subkey: "Directory\Background\shell\{#MyAppName}"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#MyAppExeName}"""; Flags: uninsdeletekey
; Explicitly pass the Explorer directory. Appending \. also handles drive roots
; whose trailing backslash would otherwise escape the closing command-line quote.
Root: HKCR; Subkey: "Directory\Background\shell\{#MyAppName}\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --clipboard-dir ""%V\."""; Flags: uninsdeletekey

Root: HKCR; Subkey: "SystemFileAssociations\.md\shell\{#MyAppName}"; ValueType: string; ValueData: "转换为图片 (原生引擎)"; Flags: uninsdeletekey
Root: HKCR; Subkey: "SystemFileAssociations\.md\shell\{#MyAppName}"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#MyAppExeName}"""; Flags: uninsdeletekey
Root: HKCR; Subkey: "SystemFileAssociations\.md\shell\{#MyAppName}\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" -- ""%1"""; Flags: uninsdeletekey
