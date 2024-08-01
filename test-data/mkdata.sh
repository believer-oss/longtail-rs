#!/bin/bash

wget "https://github.com/DanEngelbrecht/golongtail/releases/download/v0.4.3/longtail-linux-x64"
mv longtail-linux-x64 longtail
chmod +x longtail

rm -rf small medium large

mkdir small
pushd small || exit

rm -rf local-store-index/ storage/ target-path/ testdir/

mkdir testdir
echo -n "test" >testdir/testfile

../longtail \
  upsync \
  --source-path testdir \
  --target-path target-path/testdir.lvi \
  --version-local-store-index-path local-store-index/testdir.lvi \
  --storage-uri storage/testdir/

echo -n "another test" >testdir/testfile

../longtail \
  upsync \
  --source-path testdir \
  --target-path target-path/testdir2.lvi \
  --version-local-store-index-path local-store-index/testdir2.lvi \
  --storage-uri storage/testdir/

# No clue why longtail isn't cleaning up this lock file on linux
rm -f storage/testdir/store.lsi.sync
popd || exit
