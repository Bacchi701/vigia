param([int]$Puerto, [string]$Token, [int]$Segundos = 300)
# Hace lo mismo que la página en la vista «Ahora»: pide /api/ahora cada 2 s.
$fin = (Get-Date).AddSeconds($Segundos)
$n = 0; $bytes = 0
while ((Get-Date) -lt $fin) {
  $r = Invoke-WebRequest -Uri "http://127.0.0.1:$Puerto/api/ahora" -Headers @{ "X-Vigia-Token" = $Token } -UseBasicParsing
  $n++; $bytes += $r.RawContentLength
  Start-Sleep -Seconds 2
}
"peticiones=$n bytes_promedio=$([math]::Round($bytes / [math]::Max($n,1)))"
