# hunt-summary-extractor

Parses the 'attributes.xml' file for post-match player details, which contains hidden information such as numerical MMR, and saves them in a timestamped CSV file.

```
Usage: hunt-summary-extractor.exe [OPTIONS]

Options:
  -i, --input <INPUT>            Path of 'attributes.xml' [default: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Hunt Showdown\\user\\profiles\\default\\attributes.xml"]
  -o, --output-dir <OUTPUT_DIR>  Path of output directory [default: ~/Documents/Hunt/MatchData]
  -s, --single                   Disable continuous mode, checking only once for file modification
  -z, --zero-based               Zero-based numbering for teams and players
      --temp-file <TEMP_FILE>    Filename for temporary CSV file [default: TEMP.CSV]
  -h, --help                     Print help
  -V, --version                  Print version
  ```
