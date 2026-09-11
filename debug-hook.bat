@echo off
echo %DATE% %TIME% called with args: %* >> "%APPDATA%\com.tauri-app.my\hook-debug.log"
echo stdin: >> "%APPDATA%\com.tauri-app.my\hook-debug.log"
type con >> "%APPDATA%\com.tauri-app.my\hook-debug.log"
echo. >> "%APPDATA%\com.tauri-app.my\hook-debug.log"
echo --- >> "%APPDATA%\com.tauri-app.my\hook-debug.log"
