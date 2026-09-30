$ErrorActionPreference = 'Stop'
$envFile = Join-Path $PSScriptRoot '.env.local'

if (-not (Test-Path -LiteralPath $envFile)) {
    throw 'Missing .env.local. Copy .env.local.example to .env.local and set the MongoDB URI.'
}

foreach ($line in Get-Content -LiteralPath $envFile) {
    $entry = $line.Trim()
    if (-not $entry -or $entry.StartsWith('#')) { continue }

    $separator = $entry.IndexOf('=')
    if ($separator -lt 1) { throw 'Invalid .env.local entry; expected NAME=value.' }

    $name = $entry.Substring(0, $separator).Trim()
    $value = $entry.Substring($separator + 1).Trim()
    [Environment]::SetEnvironmentVariable($name, $value, 'Process')
}

$env:BICERIN__SERVER__BIND = '127.0.0.1:8448'
$env:BICERIN__SERVER__SERVER_NAME = 'localhost'
$env:BICERIN__SERVER__PUBLIC_URL = 'http://localhost:8448'
$env:BICERIN__DATABASE__BACKEND = 'mongodb'
$env:BICERIN__DATABASE__MONGO_DATABASE = 'bicerin_matrix_test'
$env:BICERIN__MATRIX__REGISTRATION_ENABLED = 'true'
$env:BICERIN__MEDIA__LOCAL_PATH = '.local-data/media'

Push-Location $PSScriptRoot
try {
    cargo run -p bicerin-server
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
