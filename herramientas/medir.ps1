param([int]$ProcId, [int]$Segundos = 300, [string]$Nombre = "medicion")
# Mide CPU y memoria de un proceso durante $Segundos (solo lectura).
$p = Get-Process -Id $ProcId
$n = [Environment]::ProcessorCount
$cpu0 = $p.TotalProcessorTime.TotalSeconds
$t0 = Get-Date
$ws = @(); $priv = @()
while (((Get-Date) - $t0).TotalSeconds -lt $Segundos) {
  Start-Sleep -Seconds 10
  $p.Refresh()
  $ws += $p.WorkingSet64
  $priv += $p.PrivateMemorySize64
}
$p.Refresh()
$dt = ((Get-Date) - $t0).TotalSeconds
$cpu = ($p.TotalProcessorTime.TotalSeconds - $cpu0) / $dt / $n * 100
$r = [ordered]@{
  nombre = $Nombre
  segundos = [math]::Round($dt)
  procesadores = $n
  cpu_pct_equipo = [math]::Round($cpu, 3)
  working_set_mb_prom = [math]::Round((($ws | Measure-Object -Average).Average) / 1MB, 1)
  working_set_mb_max = [math]::Round((($ws | Measure-Object -Maximum).Maximum) / 1MB, 1)
  privada_mb_prom = [math]::Round((($priv | Measure-Object -Average).Average) / 1MB, 1)
  privada_mb_max = [math]::Round((($priv | Measure-Object -Maximum).Maximum) / 1MB, 1)
  hilos = $p.Threads.Count
  manijas = $p.HandleCount
}
$r | ConvertTo-Json
