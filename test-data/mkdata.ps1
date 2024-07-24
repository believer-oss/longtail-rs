Invoke-WebRequest "https://github.com/DanEngelbrecht/golongtail/releases/download/v0.4.3/longtail-win32-x64.exe" -OutFile "longtail.exe"

Remove-Item -Recurse -Force "small"
Remove-Item -Recurse -Force "medium"
Remove-Item -Recurse -Force "large"

mkdir small
cd small

mkdir testdir
$test = "test"
$test | Out-File -FilePath "testdir\testfile"
& "../longtail.exe" `
  upsync `
  --source-path "testdir" `
  --target-path "target-path/testdir.lvi" `
  --version-local-store-index-path "local-store-index/testdir.lvi" `
  --storage-uri "storage/testdir/"

$test = "another test"
$test | Out-File -FilePath "testdir\testfile"

& "../longtail.exe" `
  upsync `
  --source-path "testdir" `
  --target-path "target-path/testdir2.lvi" `
  --version-local-store-index-path "local-store-index/testdir2.lvi" `
  --storage-uri "storage/testdir/"
