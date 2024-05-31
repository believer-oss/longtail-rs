#!/bin/bash

rm -rf local-store-index/ storage/ target-path/ testdir/

mkdir testdir
echo "test" >testdir/testfile

longtail \
	upsync \
	--source-path testdir \
	--target-path target-path/testdir.lvi \
	--version-local-store-index-path local-store-index/testdir.lvi \
	--storage-uri storage/testdir/
