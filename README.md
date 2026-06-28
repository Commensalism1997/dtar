# dtar

![Screenshot](example.png)

## Syntax

```bash
dtar [-h] <SUBCOMMAND>...
```

## Installation

Provided you have cargo and have .cargo/bin in $PATH:

```bash
cargo install --git https://github.com/Commensalism1997/dtar.git
```

## Extraction variants
A progress bar requires the total number of entries to be known. For the tar format, the only way to know the total amount of entries is to either enumerate it or simply track the file being read.

By default, the latter is employed; this is called the *direct mode* and can be explicitly specified with `-m direct`.

In *processor mode*, dtar spawns two threads: one reads the file and counts its entries, the other extracts it in meantime and displays the progress bar once the count is finished. On computers with fast CPUs, this mode is usually way faster than the other modes.

The *memory mode* can be chosen with `-m memory`. With it the file is decompressed once and buffered in memory, which is then counted and extracted. Since counting an uncompressed tarball is trivial, the progress bar is usually available instantly once the file is read. For this, enough memory is required to contain the entire uncompressed tarball; therefore, this mode is unsuitable for computers with little memory and large files.

The *storage mode* can be chosen with `-m storage` and is exactly the same as the memory mode, except the uncompressed tarball is written to disk instead of memory. This doesn't require as much memory, but is significantly slower. `-m storage-keep` will keep the uncompressed tarball instead of deleting it once finished.

The *sync mode* can be chosen with `-m sync` and will read the file, count it, then read it again and extract. This is twice as slow as a typical extraction, but causes no additional load.

The `-n` or `--no-progress` flag will forego all of the above modes and extract the file the same way `tar -x` does.