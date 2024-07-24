Invoke-WebRequest "https://github.com/DanEngelbrecht/golongtail/releases/download/v0.4.3/longtail-win32-x64.exe" -OutFile "longtail.exe"

Remove-Item -Recurse -Force "small" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "medium" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "large" -ErrorAction SilentlyContinue

mkdir small
cd small

mkdir testdir
$test = "test"
$test | Out-File -Encoding ASCII -FilePath "testdir/testfile"
& "../longtail.exe" `
  upsync `
  --source-path "testdir" `
  --target-path "target-path/testdir.lvi" `
  --version-local-store-index-path "local-store-index/testdir.lvi" `
  --storage-uri "storage/testdir/"

$test = "another test"
$test | Out-File -Encoding ASCII -FilePath "testdir/testfile"
& "../longtail.exe" `
  upsync `
  --source-path "testdir" `
  --target-path "target-path/testdir2.lvi" `
  --version-local-store-index-path "local-store-index/testdir2.lvi" `
  --storage-uri "storage/testdir/"
