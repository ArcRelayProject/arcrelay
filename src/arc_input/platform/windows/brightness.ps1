$ErrorActionPreference = 'Stop'
try {
    $values = @(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Where-Object Active)
    $methods = @(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods)
    if ($env:ARCRELAY_BRIGHTNESS_MODE -eq 'probe') {
        $names = @($values | Where-Object { $methods.InstanceName -contains $_.InstanceName } | ForEach-Object { $_.InstanceName })
        ConvertTo-Json -InputObject $names -Compress
    } elseif ($env:ARCRELAY_BRIGHTNESS_MODE -eq 'adjust') {
        # Values arrive as data through environment variables, never shell code.
        $name = $env:ARCRELAY_BRIGHTNESS_INSTANCE
        $value = @($values | Where-Object { $_.InstanceName -eq $name })
        $method = @($methods | Where-Object { $_.InstanceName -eq $name })
        if ($value.Count -ne 1 -or $method.Count -ne 1) { throw 'Display is unavailable or ambiguous' }
        $steps = [Math]::Min(20, [Math]::Max(-20, [int]$env:ARCRELAY_BRIGHTNESS_STEPS))
        $brightness = [byte][Math]::Min(100, [Math]::Max(0, [int]$value[0].CurrentBrightness + $steps * 5))
        $result = Invoke-CimMethod -InputObject $method[0] -MethodName WmiSetBrightness -Arguments @{Timeout=[uint32]0;Brightness=$brightness}
        if ($result.ReturnValue -ne 0) { throw 'Brightness write failed' }
    } else { throw 'Unknown brightness operation' }
} catch { Write-Error $_; exit 1 }
