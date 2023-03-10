mod animation;
mod bounds;
mod cli;
mod config;
mod input;
mod mouse;

use bounds::Bounds;
use config::Config;
use input::KeyCommand;
use mouse::{MouseExt, PointExt};

use anyhow::{anyhow, bail, Result};
use crossterm::cursor::{MoveTo, MoveToColumn, MoveToNextLine};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor, Stylize};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use std::io::stdout;

use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let matches = cli::build().get_matches();

    let interval = *matches
        .get_one::<Duration>("INTERVAL")
        .expect("interval should be required by clap");
    let pause_interval = *matches
        .get_one::<Duration>("pause-interval")
        .unwrap_or(&interval);
    let fps = matches
        .get_one::<u32>("fps")
        .copied()
        .expect("fps should be required by clap");
    let bounds = Bounds::from(&matches);
    if bounds.has_empty_range() {
        eprintln!("error: bounds {bounds} will result in no mouse movement");
        return ExitCode::FAILURE;
    }
    let animate = !matches.get_flag("no-animate");
    let auto_pause = !matches.get_flag("no-autopause");

    let mut config = Config {
        interval,
        pause_interval,
        fps,
        bounds,
        animate,
        auto_pause,
    };

    let mut mouse = MouseExt::with_config(&config);

    let mut stdout = stdout();
    execute!(
        stdout,
        cursor::Hide,
        EnterAlternateScreen,
        Clear(ClearType::All),
    )
    .expect("should be able to execute crossterm commands");
    enable_raw_mode().expect("should be able to start raw mode");

    let code = match run(&mut mouse, &mut config) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    };

    disable_raw_mode().expect("should be able to disable raw mode");
    execute!(stdout, cursor::Show, LeaveAlternateScreen)
        .expect("should be able to leave alternate screen");

    code
}

fn run(mouse: &mut MouseExt, config: &mut Config) -> Result<()> {
    let mut stdout = stdout();

    print_header(&mut stdout);

    let rng = fastrand::Rng::new();
    let mut orig = mouse
        .pos()
        .map_err(|_| anyhow!("failed to get mouse position"))?;

    let poll_time = Duration::from_millis(25);

    let action_text = if config.animate {
        " animating to "
    } else {
        " placed cursor at "
    };

    execute!(
        stdout,
        Clear(ClearType::CurrentLine),
        Print("Status:".bold().dim()),
        MoveToColumn(0),
    )?;

    let mut last_p = orig;
    loop {
        match KeyCommand::read(&poll_time)? {
            KeyCommand::Quit => return Ok(()),
            KeyCommand::ToggleAnimate => {
                input::debounce()?;
                config.animate = !config.animate;
                mouse.toggle_animate();
            }
            KeyCommand::TogglePause => {
                execute!(
                    stdout,
                    Clear(ClearType::CurrentLine),
                    Print("Status:".bold().dim()),
                    SetForegroundColor(Color::Yellow),
                    Print(" paused"),
                    ResetColor,
                    Print(" (press ".dim()),
                    Print("p".bold()),
                    Print(" to unpause)".dim()),
                    MoveToColumn(0),
                )?;
                input::debounce()?;
                'pause: loop {
                    match KeyCommand::read(&Duration::from_secs(60))? {
                        KeyCommand::Quit => return Ok(()),
                        KeyCommand::TogglePause => {
                            input::debounce()?;
                            break 'pause;
                        }
                        _ => (),
                    }
                }
            }
            _ => (),
        };

        let p = sample_point(&rng, &config.bounds, orig, last_p);
        execute!(
            stdout,
            Clear(ClearType::CurrentLine),
            Print("Status:".bold().dim()),
            Print(action_text.dim()),
            SetForegroundColor(Color::Cyan),
            Print(p),
            ResetColor,
            MoveToColumn(0),
        )?;

        match mouse.move_to(p) {
            Ok(_) => (),
            Err(err) => match err {
                mouse::MouseError::Busy => {
                    let pause_str = format!("{:.1}s", config.pause_interval.as_secs_f32());
                    execute!(
                        stdout,
                        Clear(ClearType::CurrentLine),
                        Print("Status:".bold().dim()),
                        Print(" auto-pausing for ".dim()),
                        SetForegroundColor(Color::Yellow),
                        Print(pause_str),
                        ResetColor,
                        MoveToColumn(0),
                    )?;
                    mouse.auto_pause();
                    if config.bounds.is_relative() {
                        // use the new position as origin since it was moved
                        orig = mouse
                            .pos()
                            .map_err(|_| anyhow!("failed to get mouse position"))?;
                    }
                }
                e => bail!("failed to move mouse ({e})"),
            },
        }

        last_p = p;
    }
}

fn sample_point(
    rng: &fastrand::Rng,
    bounds: &Bounds,
    orig: PointExt,
    last_p: PointExt,
) -> PointExt {
    loop {
        let result = match *bounds {
            Bounds::Rect { x1, y1, x2, y2 } => {
                let x_range = if x1 <= x2 { x1..=x2 } else { x2..=x1 };
                let y_range = if y1 <= y2 { y1..=y2 } else { y2..=y1 };
                PointExt {
                    x: rng.i32(x_range),
                    y: rng.i32(y_range),
                }
            }
            Bounds::Relative { dx: x, dy: y } => PointExt {
                x: rng.i32((orig.x - x)..=(orig.x + x)),
                y: rng.i32((orig.y - y)..=(orig.y + y)),
            },
        };

        if result != last_p {
            return result;
        }
    }
}

fn print_header(stdout: &mut std::io::Stdout) {
    execute!(
        stdout,
        MoveTo(0, 0),
        Print("Application started.".dim()),
        MoveToNextLine(2),
        Print("Commands:".bold().dim()),
        MoveToNextLine(1),
        Print("    Press ".dim()),
        Print("q".bold()),
        Print(" to quit".dim()),
        MoveToNextLine(1),
        Print("    Press ".dim()),
        Print("p".bold()),
        Print(" to toggle pause".dim()),
        MoveToNextLine(1),
        Print("    Press ".dim()),
        Print("a".bold()),
        Print(" to toggle animations".dim()),
        MoveToNextLine(1),
        Print("    Press any other key to skip an iteration".dim()),
        MoveToNextLine(2),
    )
    .expect("should be able to write to stdout");
}
