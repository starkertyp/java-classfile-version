# java-classfile-version

This tool will try to extract the required minimal java version for a given class file or a given jar. The version will be printed to STDOUT.
It supports setting a maximum version by passing `--max` (see below). If this is set and the required minimal version surpasses the given maximum,
the command will exit with a code > 0.

This supports multiple files at once by passing more than one file, for example with a glob pattern

```sh
java-classfile-version /some/project/target/*.jar
```

## Usage

```
Usage: java-classfile-version [OPTIONS] <path>...

Arguments:
  <path>...  files to read

Options:
  -m, --max <MAXIMUM>  maximum version that is supported by your use case. A version higher than that will result in an exit code > 0
  -v, --verbose...     verbose logging. can be set multiple times
  -h, --help           Print help
  -V, --version        Print version
  
```
