use chrono::prelude::*;
use clap::Parser;
use directories::UserDirs;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use quick_xml::de::from_str;
use regex::Regex;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Extracts Hunt: Showdown player match data from 'attributes.xml' into a CSV file
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path of 'attributes.xml'
    #[arg(
        short,
        long,
        default_value = r"C:\Program Files (x86)\Steam\steamapps\common\Hunt Showdown\user\profiles\default\attributes.xml"
    )]
    input: String,

    /// Path of output directory [default: ~/Documents/Hunt/MatchData]
    #[arg(short, long)]
    output_dir: Option<String>,

    /// Disable continuous mode, checking only once for file modification
    #[arg(short, long)]
    single: bool,

    /// Zero-based numbering for teams and players
    #[arg(short, long)]
    zero_based: bool,

    /// Filename for temporary CSV file
    #[arg(long, default_value = "TEMP.CSV")]
    temp_file: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "Attributes")]
struct Attributes {
    #[serde(default, rename = "Attr")]
    items: Vec<Item>,
}

#[derive(Deserialize, Debug, Clone)]
struct Item {
    #[serde(rename = "@name")]
    name: String,

    #[serde(rename = "@value")]
    value: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let user_dir = UserDirs::new();
    let output_dir_path = match &args.output_dir {
        Some(p) => PathBuf::from(p),
        None => match &user_dir {
            Some(ud) => ud.document_dir().unwrap().join("Hunt").join("MatchData"),
            None => {
                panic!("Could not obtain handle to Home directory")
            }
        },
    };

    if !args.single {
        println!("Watching for changes to 'attributes.xml'...");
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_secs(2), None, tx).unwrap();
        debouncer
            .watcher()
            .watch(args.input.as_ref(), RecursiveMode::Recursive)?;

        for res in rx {
            match res {
                Ok(_) => extract_player_data(&args, output_dir_path.as_path())?,
                Err(e) => println!("watch error: {e:?}"),
            }
        }
    } else {
        extract_player_data(&args, output_dir_path.as_path())?;
    }

    Ok(())
}

fn extract_player_data<P: AsRef<Path>>(
    args: &Args,
    output_dir_path: P,
) -> Result<(), Box<dyn Error>> {
    let re_player_entry = Regex::new(r"MissionBagPlayer_(\d+)_(\d+)_(\w+)")?;

    let contents = fs::read_to_string(&args.input).expect("Could not open file.");
    let attr: Attributes = from_str(contents.as_str()).unwrap();

    let output_file_path = PathBuf::from(output_dir_path.as_ref()).join(&args.temp_file);

    fs::create_dir_all(&output_dir_path).expect("Could not create output directory.");

    // Grab a reference to the latest existing CSV file, if it exists, for comparison later
    let mut existing_files: Vec<fs::DirEntry> = fs::read_dir(&output_dir_path)
        .expect("Could not access output directory")
        .filter(|r| match r {
            Ok(de) => {
                de.metadata().unwrap().is_file()
                    && de.path().extension().unwrap() == "csv"
                    && de.file_name() != args.temp_file.as_str()
            }
            _ => false,
        })
        .flatten()
        .collect();
    existing_files.sort_by_cached_key(|f| f.metadata().unwrap().modified().unwrap());
    let latest_csv = existing_files.last();

    // Iterate until the match's team count is found, output the player data, then break

    for item in attr.items.iter() {
        if item.name == "MissionBagNumTeams" {
            let num_teams: u32 = item.value.parse()?;

            // Filter only player data that falls within team count into new list

            let player_entries: Vec<Item> = attr
                .items
                .into_iter()
                .filter(|i| match re_player_entry.captures(i.name.as_str()) {
                    Some(c) => c.get(1).unwrap().as_str().parse::<u32>().unwrap() < num_teams,
                    None => false,
                })
                .collect();

            let temp_file = fs::File::options()
                .read(true)
                .write(true)
                .create(true)
                .open(&output_file_path)?;
            let mut temp_file = BufWriter::new(temp_file);

            // Output headers

            temp_file.write_all(b"Team,Player")?;
            print!("Team,Player");
            for item in player_entries.iter() {
                let captures = re_player_entry.captures(item.name.as_str()).unwrap();

                let team = captures.get(1).unwrap().as_str().parse::<u32>()?;
                let player = captures.get(2).unwrap().as_str().parse::<u32>()?;
                let header = captures.get(3).unwrap();

                // Break when the player changes. We only want the headers once
                let current_player = (3 * team) + player;
                if current_player != 0 {
                    break;
                }

                temp_file.write_all(format!(",{}", header.as_str()).as_bytes())?;
                print!(",{}", captures.get(3).unwrap().as_str());
            }

            // Output player data

            let mut previous_player = u32::MAX;
            let mut skip_to_team = 0;
            for item in player_entries {
                let captures = re_player_entry.captures(item.name.as_str()).unwrap();

                let team = captures.get(1).unwrap().as_str().parse::<u32>()?;
                let player = captures.get(2).unwrap().as_str().parse::<u32>()?;

                if team < skip_to_team {
                    continue;
                }

                // Begin a new row whenever the player changes
                let current_player = (3 * team) + player;
                if current_player != previous_player {
                    previous_player = current_player;

                    // Skip to next team if player slot is empty
                    if item.value.is_empty() {
                        skip_to_team = team + 1;
                        continue;
                    }
                    let team_output = team + if args.zero_based { 0 } else { 1 };
                    let player_output = player + if args.zero_based { 0 } else { 1 };
                    temp_file.write_all(format!("\n{team_output},{player_output}").as_bytes())?;
                    print!("\n{team_output},{player_output}");
                }

                temp_file.write_all(format!(",{}", item.value).as_bytes())?;
                print!(",{}", item.value);
            }
            println!();

            break;
        }
    }

    // If the existing latest CSV file matches the newly created one, or if it does not exist,
    // then rename temp file with a timestamp
    if match latest_csv {
        Some(de) => {
            let existing_contents =
                fs::read_to_string(de.path()).expect("Could not read existing latest CSV file.");
            let new_contents = fs::read_to_string(&output_file_path)
                .expect("Could not read newly created temporary CSV file.");

            new_contents != existing_contents
        }
        None => true,
    } {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let final_path = output_dir_path.as_ref().join(format!("{timestamp}.csv"));
        fs::rename(
            output_file_path,
            &final_path
        )
        .expect("Could not rename temporary CSV file with timestamp.");
        println!("New player summary saved: '{}'", final_path.to_string_lossy());
    }

    Ok(())
}
