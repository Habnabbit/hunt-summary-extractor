use chrono::prelude::*;
use clap::Parser;
use directories::UserDirs;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use quick_xml::de::from_str;
use serde::Deserialize;
use std::collections::HashMap;
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

const HEADERS: [&str; 17] = [
    "blood_line_name",
    "mmr",
    "skillbased",
    "downedbyme",
    "killedbyme",
    "downedbyteammate",
    "killedbyteammate",
    "downedme",
    "killedme",
    "downedteammate",
    "killedteammate",
    "proximitytome",
    "proximitytoteammate",
    "bountypickedup",
    "bountyextracted",
    "teamextraction",
    "profileid",
];

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
    let contents = fs::read_to_string(&args.input).expect("Could not open file.");
    let attributes: Attributes = from_str(contents.as_str()).unwrap();

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

    // Build map of names to values from attributes file
    let mut attr_map = HashMap::new();
    for item in attributes.items.iter() {
        attr_map.insert(&item.name, &item.value);
    }

    // Check if attributes file has team data, and get the number of teams
    if let Some(num_teams) = attr_map.get(&"MissionBagNumTeams".to_string()) {
        let temp_file = fs::File::options()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open(&output_file_path)?;
        let mut temp_file = BufWriter::new(temp_file);

        // Write CSV header row
        temp_file.write_all(format!("Team,Player,{}", HEADERS.join(",")).as_bytes())?;

        // Get number of players in each team
        let mut num_players = Vec::new();
        for team in 0..num_teams.parse::<u32>()? {
            num_players.push(
                attr_map
                    .get(&format!("MissionBagTeam_{team}_numplayers"))
                    .unwrap()
                    .parse::<u32>()?,
            );
        }

        // Iterate over players in each team, collecting attributes that exist in HEADERS array
        for (team, &team_size) in num_players.iter().enumerate() {
            for player in 0..team_size {
                let team_output = team + if args.zero_based { 0 } else { 1 };
                let player_output = player + if args.zero_based { 0 } else { 1 };
                temp_file.write_all(format!("\n{team_output},{player_output}").as_bytes())?;

                for header in HEADERS {
                    let value = *attr_map
                        .get(&format!("MissionBagPlayer_{team}_{player}_{header}"))
                        .unwrap();

                    temp_file.write_all(format!(",{value}").as_bytes())?;
                }
            }
        }
    }

    // If the existing latest CSV file matches the newly created one, or if it does not exist,
    // then rename temp file with a timestamp
    let new_contents = fs::read_to_string(&output_file_path)
        .expect("Could not read newly created temporary CSV file.");
    if match latest_csv {
        Some(de) => {
            let existing_contents =
                fs::read_to_string(de.path()).expect("Could not read existing latest CSV file.");

            new_contents != existing_contents
        }
        None => true,
    } {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let final_path = output_dir_path.as_ref().join(format!("{timestamp}.csv"));
        fs::rename(output_file_path, &final_path)
            .expect("Could not rename temporary CSV file with timestamp.");
        println!("{new_contents}");
        println!(
            "New player summary saved: '{}'",
            final_path.to_string_lossy()
        );
    }

    Ok(())
}
