use std::{
    error::Error,
    fmt::Display,
    io::{Read, stdin},
};

use clap::{Args, Parser, Subcommand, arg, command};
use gcode::{Callbacks, GCode, Line, Mnemonic, Span, Word, full_parse_with_callbacks};
use serde::{Deserialize, Serialize};

struct GcodeError;

impl Callbacks for GcodeError {} // TODO

#[derive(Debug, Default, Serialize, Deserialize)]
struct Extent {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

#[derive(Debug)]
enum GctkError {
    UnsupportedCommand(GCode),
    EmptyExtent,
    UnknownPosition(usize),
}

impl Display for GctkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GctkError::UnsupportedCommand(gcode) => {
                write!(f, "Found unsupported command {}", gcode.major_number())
            }
            GctkError::EmptyExtent => write!(
                f,
                "Found empty extent (maybe input contains no movement commands on some axis?)"
            ),
            GctkError::UnknownPosition(line_num) => write!(
                f,
                "Found relative motion command with unknown absolute position on line number {line_num}"
            ),
        }
    }
}

impl Error for GctkError {}

enum PositioningMode {
    Absolute,
    Relative,
}

fn get_xy_extent(lines: &[Line]) -> Result<Extent, GctkError> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (None, None, None, None);
    let mut position = Point3::zero();
    let mut positioning_mode = PositioningMode::Absolute;
    for (line_idx, line) in lines.iter().enumerate() {
        for command in line
            .gcodes
            .iter()
            .filter(|c| c.mnemonic == Mnemonic::General)
        {
            match command.major_number() {
                0 | 1 => {
                    if let Some(x) = command.value_for('X') {
                        match (&positioning_mode, min_x) {
                            (PositioningMode::Absolute, Some(m)) => {
                                if x < m {
                                    min_x = Some(x)
                                }
                            }
                            (PositioningMode::Relative, Some(m)) => {
                                position.x += x;
                                if position.x < m {
                                    min_x = Some(position.x);
                                }
                            }
                            (PositioningMode::Absolute, None) => min_x = Some(x),
                            (PositioningMode::Relative, None) => {
                                return Err(GctkError::UnknownPosition(line_idx + 1));
                            }
                        }
                        match (&positioning_mode, max_x) {
                            (PositioningMode::Absolute, Some(m)) => {
                                if x > m {
                                    max_x = Some(x)
                                }
                            }
                            (PositioningMode::Relative, Some(m)) => {
                                position.x += x;
                                if position.x > m {
                                    max_x = Some(position.x);
                                }
                            }
                            (PositioningMode::Absolute, None) => max_x = Some(x),
                            (PositioningMode::Relative, None) => {
                                return Err(GctkError::UnknownPosition(line_idx + 1));
                            }
                        }
                    }
                    if let Some(y) = command.value_for('Y') {
                        match (&positioning_mode, min_y) {
                            (PositioningMode::Absolute, Some(m)) => {
                                if y < m {
                                    min_y = Some(y)
                                }
                            }
                            (PositioningMode::Relative, Some(m)) => {
                                position.y += y;
                                if position.y < m {
                                    min_y = Some(position.y);
                                }
                            }
                            (PositioningMode::Absolute, None) => min_y = Some(y),
                            (PositioningMode::Relative, None) => {
                                return Err(GctkError::UnknownPosition(line_idx + 1));
                            }
                        }
                        match (&positioning_mode, max_y) {
                            (PositioningMode::Absolute, Some(m)) => {
                                if y > m {
                                    max_y = Some(y)
                                }
                            }
                            (PositioningMode::Relative, Some(m)) => {
                                position.y += y;
                                if position.y > m {
                                    max_y = Some(position.y);
                                }
                            }
                            (PositioningMode::Absolute, None) => max_y = Some(y),
                            (PositioningMode::Relative, None) => {
                                return Err(GctkError::UnknownPosition(line_idx + 1));
                            }
                        }
                    }
                }
                90 => positioning_mode = PositioningMode::Absolute,
                91 => positioning_mode = PositioningMode::Relative,
                4 | 21 | 64 | 94 => (),
                _ => return Err(GctkError::UnsupportedCommand(command.clone())),
            };
        }
    }
    Ok(Extent {
        min_x: min_x.ok_or(GctkError::EmptyExtent)?,
        min_y: min_y.ok_or(GctkError::EmptyExtent)?,
        max_x: max_x.ok_or(GctkError::EmptyExtent)?,
        max_y: max_y.ok_or(GctkError::EmptyExtent)?,
    })
}

struct Point3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Point3 {
    fn zero() -> Point3 {
        Point3 {
            x: 0.,
            y: 0.,
            z: 0.,
        }
    }
}

fn translate(lines: &mut [Line], offset: &Point3) -> Result<(), GctkError> {
    for line in lines.iter_mut() {
        for command in line
            .gcodes
            .iter_mut()
            .filter(|c| c.mnemonic == Mnemonic::General)
        {
            match command.major_number() {
                0 | 1 | 2 => {
                    for argument in command.arguments.iter_mut() {
                        match argument.letter.to_ascii_uppercase() {
                            'X' => argument.value += offset.x,
                            'Y' => argument.value += offset.y,
                            'Z' => argument.value += offset.z,
                            _ => (),
                        }
                    }
                }
                4 | 21 | 64 | 90 | 91 | 94 => (),
                _ => return Err(GctkError::UnsupportedCommand(command.clone())),
            };
        }
    }
    Ok(())
}

enum MirrorAxis {
    X,
    Y,
    Z,
}

impl From<&MirrorAxis> for char {
    fn from(value: &MirrorAxis) -> Self {
        match value {
            MirrorAxis::X => 'X',
            MirrorAxis::Y => 'Y',
            MirrorAxis::Z => 'Z',
        }
    }
}

fn mirror(lines: &mut [Line], axis: MirrorAxis, value: f32) -> Result<(), GctkError> {
    for line in lines.iter_mut() {
        for command in line
            .gcodes
            .iter_mut()
            .filter(|c| c.mnemonic == Mnemonic::General)
        {
            match command.major_number() {
                0 | 1 => {
                    for argument in command.arguments.iter_mut() {
                        if argument.letter == (&axis).into() {
                            argument.value = 2. * value - argument.value;
                        }
                    }
                }
                2 => {
                    for argument in command.arguments.iter_mut() {
                        match (argument.letter, &axis) {
                            (l, a) if l == a.into() => argument.value = 2. * value - argument.value,
                            ('I', MirrorAxis::X) | ('J', MirrorAxis::Y) => {
                                argument.value *= -1.;
                            }
                            (_, _) => (),
                        }
                    }
                }
                91 => {
                    for argument in command.arguments.iter_mut() {
                        if argument.letter == (&axis).into() {
                            argument.value *= -1.;
                        }
                    }
                }
                4 | 21 | 64 | 90 | 94 => (),
                _ => return Err(GctkError::UnsupportedCommand(command.clone())),
            };
        }
    }
    Ok(())
}

type _Mesh = Vec<Point3>;

fn _mesh_level(lines: &mut [Line], _mesh: _Mesh, _num_neighbors: usize) -> Result<(), GctkError> {
    let mut current_x = None;
    let mut current_y = None;
    let mut current_z = None;
    for line in lines.iter_mut() {
        for command in line
            .gcodes
            .iter_mut()
            .filter(|c| c.mnemonic == Mnemonic::General)
        {
            match command.major_number() {
                0 | 1 => {
                    let (mut command_x, mut command_y, mut command_z) = (None, None, None);
                    for argument in command.arguments.iter() {
                        match argument.letter {
                            'X' => command_x = Some(argument.value),
                            'Y' => command_y = Some(argument.value),
                            'Z' => command_z = Some(argument.value),
                            _ => (),
                        };
                    }
                    if command_x.is_some() {
                        current_x = command_x;
                    }
                    if command_y.is_some() {
                        current_y = command_y;
                    }
                    if command_z.is_some() {
                        current_z = command_z;
                    }
                    if current_x.is_some() && current_y.is_some() && current_z.is_some() {
                        let (_x, _y, mut new_z) =
                            (current_x.unwrap(), current_y.unwrap(), current_z.unwrap());
                        new_z *= -1.; // TODO estimate adjusted Z value using mesh values and current position
                        if command_z.is_some() {
                            for argument in command.arguments.iter_mut() {
                                if let 'Z' = argument.letter {
                                    argument.value = new_z;
                                }
                            }
                        } else {
                            command
                                .push_argument(Word::new('Z', new_z, Span::PLACEHOLDER))
                                .unwrap();
                        }
                    }
                }
                4 | 21 | 64 | 90 | 94 => (),
                _ => return Err(GctkError::UnsupportedCommand(command.clone())),
            };
        }
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
/// G-code Toolkit
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    GetExtent,
    Translate {
        #[arg(short, default_value_t = 0., allow_negative_numbers = true)]
        x: f32,
        #[arg(short, default_value_t = 0., allow_negative_numbers = true)]
        y: f32,
        #[arg(short, default_value_t = 0., allow_negative_numbers = true)]
        z: f32,
    },
    Mirror {
        #[command(flatten)]
        mirror_axis: MirrorArgGroup,
    },
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct MirrorArgGroup {
    #[arg(short, allow_negative_numbers = true)]
    x: Option<f32>,
    #[arg(short, allow_negative_numbers = true)]
    y: Option<f32>,
    #[arg(short, allow_negative_numbers = true)]
    z: Option<f32>,
}

fn print_lines(lines: &[Line]) {
    // TODO impl Display for Line
    for line in lines.iter() {
        for command in line.gcodes.iter() {
            println!("{}", command);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse args
    let args = Cli::parse();

    // Read input
    let mut src = String::new();
    stdin().read_to_string(&mut src)?;
    let callbacks = GcodeError;
    let mut lines: Vec<Line> = full_parse_with_callbacks(&src, callbacks).collect();

    // Apply transformation and print output
    match args.command {
        Commands::GetExtent => {
            let extent = get_xy_extent(&lines)?;
            println!("{}", serde_json::to_string(&extent)?);
        }
        Commands::Translate { x, y, z } => {
            translate(&mut lines, &Point3 { x, y, z })?;
            print_lines(&lines);
        }
        Commands::Mirror { mirror_axis } => {
            match (mirror_axis.x, mirror_axis.y, mirror_axis.z) {
                (Some(x), _, _) => mirror(&mut lines, MirrorAxis::X, x)?,
                (_, Some(y), _) => mirror(&mut lines, MirrorAxis::Y, y)?,
                (_, _, Some(z)) => mirror(&mut lines, MirrorAxis::Z, z)?,
                (None, None, None) => unreachable!("All mirror arguments are None"),
            };
            print_lines(&lines);
        }
    }
    Ok(())
}
