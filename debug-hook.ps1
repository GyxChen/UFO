$log = "$env:APPDATA\com.tauri-app.my\hook-debug.log"
$stdin = [Console]::In.ReadToEnd()
$stamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
Add-Content -Path $log -Value "[$stamp] args: $($args -join ' ')"
Add-Content -Path $log -Value "[$stamp] stdin: $stdin"
Add-Content -Path $log -Value "---"
