use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::editor::{BUTTON_IDLE, button_label, top_button};

/// Cartella dei fotogrammi grezzi, svuotata a ogni nuova registrazione.
const FRAMES_DIR: &str = "registrazione";
/// Fotogrammi al secondo del filmato. Salvare un PNG per ogni frame renderizzato
/// costerebbe piu' della simulazione stessa, quindi si campiona.
const RECORDING_FPS: f32 = 20.0;
const RECORDING_COLOR: Color = Color::srgb(0.70, 0.15, 0.15);

/// Registrazione in corso: conta i fotogrammi gia' scritti e il tempo mancante
/// al prossimo.
#[derive(Resource, Default)]
pub struct Recording {
    active: bool,
    frames: u32,
    countdown: f32,
}

impl Recording {
    pub fn start(&mut self) {
        let folder = frames_dir();
        let _ = std::fs::remove_dir_all(&folder);

        if let Err(error) = std::fs::create_dir_all(&folder) {
            error!("non riesco a creare {}: {error}", folder.display());
            return;
        }

        self.active = true;
        self.frames = 0;
        self.countdown = 0.0;
        info!("registrazione avviata");
    }

    /// Chiude la registrazione e monta il filmato. L'attesa e' visibile: ffmpeg
    /// gira qui, non in sottofondo, ma dura quanto un paio di respiri.
    pub fn stop(&mut self) {
        self.active = false;

        if self.frames == 0 {
            warn!("nessun fotogramma da montare");
            return;
        }

        let folder = frames_dir();
        let video = PathBuf::from(format!("simulazione-{}.mp4", now()));

        let outcome = Command::new("ffmpeg")
            .args(["-y", "-framerate", &RECORDING_FPS.to_string(), "-i"])
            .arg(folder.join("frame_%06d.png"))
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&video)
            .output();

        match outcome {
            Ok(done) if done.status.success() => {
                info!(
                    "filmato pronto: {} ({} fotogrammi)",
                    video.display(),
                    self.frames
                );
                let _ = std::fs::remove_dir_all(&folder);
            }
            Ok(done) => error!(
                "ffmpeg ha rifiutato il montaggio: {}",
                String::from_utf8_lossy(&done.stderr)
                    .lines()
                    .next_back()
                    .unwrap_or("nessun dettaglio")
            ),
            // Senza ffmpeg i fotogrammi restano dove sono: sono comunque il
            // materiale della registrazione, e si montano a mano.
            Err(error) => error!(
                "ffmpeg non disponibile ({error}): i {} fotogrammi restano in {}",
                self.frames,
                folder.display()
            ),
        }
    }
}

fn frames_dir() -> PathBuf {
    PathBuf::from(FRAMES_DIR)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[derive(Component)]
struct RecordButton;

#[derive(Component)]
struct RecordLabel;

pub struct RecorderPlugin;

impl Plugin for RecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Recording>()
            .add_systems(Startup, setup_record_button)
            .add_systems(Update, (toggle_recording, capture_frames, refresh_button))
            // In Last, prima che il ciclo si chiuda: chiudere la finestra
            // mentre si registra non deve buttare via i fotogrammi raccolti.
            .add_systems(Last, finish_on_exit);
    }
}

fn setup_record_button(mut commands: Commands) {
    commands.spawn((
        top_button(3),
        BackgroundColor(BUTTON_IDLE),
        RecordButton,
        children![(button_label("Registra"), RecordLabel)],
    ));
}

fn toggle_recording(
    buttons: Query<&Interaction, (Changed<Interaction>, With<RecordButton>)>,
    mut recording: ResMut<Recording>,
) {
    if !buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }

    if recording.active {
        recording.stop();
    } else {
        recording.start();
    }
}

/// Scrive un fotogramma alla cadenza scelta. La cattura e' asincrona: Bevy
/// consegna l'immagine quando il rendering del frame e' finito.
fn capture_frames(mut commands: Commands, time: Res<Time>, mut recording: ResMut<Recording>) {
    if !recording.active {
        return;
    }

    recording.countdown -= time.delta_secs();
    if recording.countdown > 0.0 {
        return;
    }
    recording.countdown += 1.0 / RECORDING_FPS;

    let path = frames_dir().join(format!("frame_{:06}.png", recording.frames));
    recording.frames += 1;

    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

fn finish_on_exit(mut exits: MessageReader<AppExit>, mut recording: ResMut<Recording>) {
    if exits.read().next().is_some() && recording.active {
        recording.stop();
    }
}

fn refresh_button(
    recording: Res<Recording>,
    mut buttons: Query<&mut BackgroundColor, With<RecordButton>>,
    mut labels: Query<&mut Text, With<RecordLabel>>,
) {
    if !recording.is_changed() {
        return;
    }

    for mut background in buttons.iter_mut() {
        background.0 = if recording.active {
            RECORDING_COLOR
        } else {
            BUTTON_IDLE
        };
    }
    for mut label in labels.iter_mut() {
        label.0 = if recording.active { "Stop" } else { "Registra" }.to_string();
    }
}
