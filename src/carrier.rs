use bevy::prelude::*;
use bevy::text::TextBounds;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::divert::{Divert, LANE_HEIGHT};
use crate::gate::{self, Gate};
use crate::geometry::circle_touches_box;
use crate::piece::{Facing, PIECE_SIZE};
use crate::reverser::{Reverser, TURN_RADIUS};
use crate::simulation::SimulationState;
use crate::switch::Switch;
use crate::turner::Turner;

pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const CARRIER_RADIUS: f32 = 15.0;
/// Il collare: la parte stretta che sporge sopra la base e regge la provetta.
/// E' quello che vedono i sensori, e siccome e' piu' stretto della base due
/// carrier a contatto restano due cose distinte anche per una fotocellula: fra
/// un collare e l'altro il fascio torna libero.
pub const COLLAR_RADIUS: f32 = 9.0;
pub const CARRIER_THICKNESS: f32 = 3.0;
/// Distanza minima fra i centri di due carrier: diametro piu' un po' di margine.
pub const CARRIER_SIZE: f32 = CARRIER_RADIUS * 2.0 + 4.0;

/// Lunghezza massima di un identificativo di campione.
pub const SAMPLE_ID_MAX_LEN: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CarrierType {
    Empty,
    WithTube,
}

/// Identificativo del campione trasportato. Il limite di 24 caratteri sta nel
/// costruttore invece che in un commento: un valore piu' lungo non puo' esistere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleId(String);

impl SampleId {
    pub fn new(text: &str) -> Option<Self> {
        // Si contano i caratteri, non i byte: un accento occupa due byte ma
        // resta un carattere solo.
        let length = text.chars().count();

        (1..=SAMPLE_ID_MAX_LEN)
            .contains(&length)
            .then(|| SampleId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Verso di marcia lungo gli assi della griglia.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Heading {
    Left,
    Right,
    Up,
    Down,
}

impl Heading {
    pub fn as_vec(self) -> Vec2 {
        match self {
            Heading::Left => Vec2::NEG_X,
            Heading::Right => Vec2::X,
            Heading::Up => Vec2::Y,
            Heading::Down => Vec2::NEG_Y,
        }
    }

    /// La destra di chi marcia, non la destra dello schermo: chi va a sinistra
    /// svolta verso l'alto, chi sale svolta verso destra.
    pub fn turn_right(self) -> Self {
        match self {
            Heading::Left => Heading::Up,
            Heading::Up => Heading::Right,
            Heading::Right => Heading::Down,
            Heading::Down => Heading::Left,
        }
    }

    pub fn turn_left(self) -> Self {
        self.turn_right().opposite()
    }

    /// Rotazione che porta una figura disegnata verso l'alto a puntare in
    /// questo verso: e' cosi' che la freccia dentro i quadrati segue l'oggetto.
    pub fn rotation(self) -> Quat {
        let along = self.as_vec();

        Quat::from_rotation_z(along.y.atan2(along.x) - std::f32::consts::FRAC_PI_2)
    }

    pub fn opposite(self) -> Self {
        match self {
            Heading::Left => Heading::Right,
            Heading::Right => Heading::Left,
            Heading::Up => Heading::Down,
            Heading::Down => Heading::Up,
        }
    }
}

/// Come si sta muovendo il carrier in questo momento. Non e' un ricordo del
/// passato ma il suo stato di moto: gli oggetti che incontra lo cambiano, e
/// senza incontri prosegue com'e'.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Motion {
    Straight(Heading),
    /// Sta percorrendo un arco in senso antiorario attorno a `centre` per altri
    /// `remaining` radianti, dopodiche' riparte dritto verso `exit`.
    Turning {
        centre: Vec2,
        remaining: f32,
        exit: Heading,
    },
}

impl Motion {
    /// Verso di marcia lungo gli assi, se non e' in curva.
    pub fn heading(self) -> Option<Heading> {
        match self {
            Motion::Straight(heading) => Some(heading),
            Motion::Turning { .. } => None,
        }
    }
}

/// Scarto trasversale entro cui il carrier e' sulla stessa linea dell'oggetto.
const REACH_ACROSS: f32 = 4.0;

/// Vero se il carrier, compiendo `step`, attraversa in questo frame il centro
/// dell'oggetto. Gli oggetti che dirottano il carrier agiscono in quell'istante:
/// prenderlo prima vorrebbe dire spostarlo di scatto sulla nuova traiettoria.
///
/// Il confronto e' un attraversamento e non una vicinanza: chi e' fermo **sul**
/// centro non lo sta attraversando, e questo e' cio' che impedisce a un oggetto
/// di riprendere il carrier che ha appena girato, mandandolo in tondo.
pub fn crosses(object: Vec3, carrier: Vec3, step: Vec3, heading: Heading) -> bool {
    let along = heading.as_vec();
    let delta = carrier.truncate() - object.truncate();

    delta.dot(along) < 0.0
        && (delta + step.truncate()).dot(along) >= 0.0
        && delta.perp_dot(along).abs() <= REACH_ACROSS
}

/// Il carrier non sa niente del percorso: sono gli oggetti che incontra a dirgli
/// dove andare. Porta pero' la propria identita', che serve anche senza interfaccia.
#[derive(Component)]
pub struct Carrier {
    pub kind: CarrierType,
    pub carrier_id: u32,
    /// Uno e uno solo, quando c'e'.
    pub sample_id: Option<SampleId>,
    pub motion: Motion,
}

/// Contatore progressivo dei carrier. Riparte da 1 a ogni avvio.
#[derive(Resource)]
pub struct NextCarrierId(u32);

impl Default for NextCarrierId {
    fn default() -> Self {
        NextCarrierId(1)
    }
}

impl NextCarrierId {
    fn take(&mut self) -> u32 {
        let id = self.0;
        self.0 = self.0.wrapping_add(1);
        id
    }
}

#[derive(Component)]
pub struct Tube;

/// La cinematica: nessun riferimento a mesh, materiali o camera, cosi' gira
/// anche senza interfaccia.
pub struct CarrierPlugin;

impl Plugin for CarrierPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NextCarrierId>().add_systems(
            Update,
            move_carrier.run_if(in_state(SimulationState::Running)),
        );
    }
}

/// L'aspetto dei carrier: si monta solo quando c'e' l'interfaccia.
pub struct CarrierVisualsPlugin;

impl Plugin for CarrierVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_carrier_assets)
            // In fila: chi cambia tipo viene prima spogliato e poi rivestito
            // nello stesso frame, senza passare un istante senza corpo.
            .add_systems(
                Update,
                (undress_changed_carriers, attach_carrier_visuals).chain(),
            );
    }
}

/// Mesh e materiali dei carrier, creati una volta sola: se venissero costruiti a
/// ogni spawn si accumulerebbero asset identici per tutta la durata del gioco.
#[derive(Resource)]
pub struct CarrierAssets {
    empty_mesh: Handle<Mesh>,
    empty_material: Handle<ColorMaterial>,
    with_tube_mesh: Handle<Mesh>,
    with_tube_material: Handle<ColorMaterial>,
    /// Il disco del collare: fa da fondo scuro al numero, che altrimenti
    /// finirebbe sul bianco della base e non si leggerebbe.
    collar_disc_mesh: Handle<Mesh>,
    /// L'anello che segna il bordo del collare sul carrier vuoto.
    collar_rim_mesh: Handle<Mesh>,
    empty_collar_material: Handle<ColorMaterial>,
    with_tube_collar_material: Handle<ColorMaterial>,
}

fn setup_carrier_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(CarrierAssets {
        // Il carrier vuoto e' un anello: cerchio senza riempimento.
        empty_mesh: meshes.add(Annulus::new(
            CARRIER_RADIUS - CARRIER_THICKNESS,
            CARRIER_RADIUS,
        )),
        empty_material: materials.add(Color::WHITE),
        with_tube_mesh: meshes.add(Circle::new(CARRIER_RADIUS)),
        with_tube_material: materials.add(Color::srgb(0.0, 0.8, 0.2)),
        collar_disc_mesh: meshes.add(Circle::new(COLLAR_RADIUS)),
        // Sottile e sul bordo esterno: dentro deve restare posto per il numero.
        collar_rim_mesh: meshes.add(Annulus::new(COLLAR_RADIUS - 1.0, COLLAR_RADIUS)),
        // Scuro ma non quanto lo sfondo: sul carrier vuoto il collare si vede
        // lo stesso, e il numero bianco che ci sta sopra si legge.
        empty_collar_material: materials.add(Color::srgb(0.13, 0.13, 0.16)),
        // Verde piu' cupo: stessa cosa sul carrier pieno.
        with_tube_collar_material: materials.add(Color::srgb(0.0, 0.55, 0.14)),
    });
}

/// Il testo viene rasterizzato a questa dimensione e poi rimpicciolito dalla
/// scala: se lo si disegnasse gia' a 3 px, zoomando resterebbe una macchia
/// sfocata, perche' lo zoom ingrandisce i pixel gia' prodotti. Cosi' invece la
/// nitidezza c'e' fino a un ingrandimento di 1/LABEL_SCALE.
const LABEL_FONT_SIZE: f32 = 32.0;
/// Un identificativo e' lungo dieci caratteri (`0x44000000`), e deve starci per
/// intero dentro il collare: da qui la misura, che e' piccola per forza. A
/// occhio nudo e' un segno; si legge ingrandendo, ed e' per questo che il testo
/// viene rasterizzato grande e poi rimpicciolito.
const LABEL_SCALE: f32 = 0.06;
/// Un identificativo occupa dieci caratteri: `0x` piu' otto cifre.
const ID_CHARS: f32 = 10.0;
/// Passo di FiraMono, il font di serie: e' a spaziatura fissa, 0.6 em per
/// carattere. Il valore viene dal file del font, non da una stima a occhio.
const MONO_ADVANCE: f32 = 0.6;
/// Larghezza del riquadro del testo: quanto basta a tenere un identificativo
/// intero su una riga sola, con un filo di margine. Piu' stretto lo manderebbe
/// a capo in mezzo alle cifre; piu' largo lo porterebbe sull'anello del
/// collare, dove sarebbe bianco su bianco.
const LABEL_BOX: f32 = ID_CHARS * MONO_ADVANCE * LABEL_FONT_SIZE * LABEL_SCALE + 0.5;

/// Di che tipo era il carrier quando gli e' stato dato un corpo. Serve ad
/// accorgersi che il tipo e' cambiato: puo' succedere, un carrier vuoto puo'
/// ricevere una provetta e uno pieno perderla.
#[derive(Component)]
struct Dressed(CarrierType);

/// Chi non e' piu' com'era disegnato viene spogliato: il sistema che veste i
/// carrier lo trova nudo e lo rifa' con l'aspetto giusto.
fn undress_changed_carriers(
    mut commands: Commands,
    carriers: Query<(Entity, &Carrier, &Dressed, Option<&Children>), Changed<Carrier>>,
) {
    for (entity, carrier, dressed, children) in carriers.iter() {
        if dressed.0 == carrier.kind {
            continue;
        }

        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        commands
            .entity(entity)
            .remove::<Mesh2d>()
            .remove::<Dressed>();
    }
}

/// Da' un corpo ai carrier appena nati. Senza questo sistema esistono lo stesso e
/// si muovono: semplicemente non li vede nessuno.
fn attach_carrier_visuals(
    mut commands: Commands,
    assets: Res<CarrierAssets>,
    carriers: Query<(Entity, &Carrier), Without<Mesh2d>>,
) {
    for (entity, carrier) in carriers.iter() {
        let (mesh, material, collar_material) = match carrier.kind {
            CarrierType::WithTube => (
                assets.with_tube_mesh.clone(),
                assets.with_tube_material.clone(),
                assets.with_tube_collar_material.clone(),
            ),
            CarrierType::Empty => (
                assets.empty_mesh.clone(),
                assets.empty_material.clone(),
                assets.empty_collar_material.clone(),
            ),
        };

        commands
            .entity(entity)
            .insert((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Dressed(carrier.kind),
            ))
            // Il collare sta fra la base e il numero: copre la base e fa da
            // fondo all'etichetta, che gli sta sopra.
            .with_child((
                Mesh2d(assets.collar_disc_mesh.clone()),
                MeshMaterial2d(collar_material),
                Transform::from_xyz(0.0, 0.0, 0.05),
            ));

        // Sul carrier vuoto il disco scuro da solo non si distinguerebbe dallo
        // sfondo: l'anello dice dove finisce il collare.
        if carrier.kind == CarrierType::Empty {
            commands.entity(entity).with_child((
                Mesh2d(assets.collar_rim_mesh.clone()),
                MeshMaterial2d(assets.empty_material.clone()),
                Transform::from_xyz(0.0, 0.0, 0.06),
            ));
        }

        commands.entity(entity).with_child(carrier_label(carrier));
    }
}

/// Etichetta dentro al cerchio: carrier_id sulla prima riga, sample_id sotto se
/// c'e'. Va a capo su qualsiasi carattere, perche' un sample_id non ha spazi in
/// cui spezzarsi.
fn carrier_label(carrier: &Carrier) -> impl Bundle {
    // In esadecimale a otto cifre, che e' la forma in cui questi
    // identificativi si leggono nell'impianto vero: `0x44000000`.
    let mut text = format!("0x{:08X}", carrier.carrier_id);
    if let Some(sample_id) = &carrier.sample_id {
        text.push('\n');
        text.push_str(sample_id.as_str());
    }

    (
        Text2d::new(text),
        TextFont {
            font_size: LABEL_FONT_SIZE,
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::new(Justify::Center, LineBreak::AnyCharacter),
        // Solo la larghezza, che serve a mandare a capo. Fissare anche
        // l'altezza allinea il testo in cima al riquadro invece di centrarlo, e
        // il numero finiva a cavallo del collare: bianco su bianco, illeggibile.
        // Senza limite in altezza il riquadro e' alto quanto il testo, quindi
        // resta centrato nel foro del collare.
        TextBounds::new_horizontal(LABEL_BOX / LABEL_SCALE),
        Transform::from_xyz(0.0, 0.0, 0.1).with_scale(Vec3::splat(LABEL_SCALE)),
    )
}

/// Immette un carrier nel flusso; il tipo (vuoto o con tubo) e' casuale 50-50.
pub fn spawn_random_carrier(
    commands: &mut Commands,
    position: Vec3,
    heading: Heading,
    ids: &mut NextCarrierId,
) {
    let mut rng = rand::rng();
    let kind = if rng.random::<u32>() > u32::MAX / 2 {
        CarrierType::WithTube
    } else {
        CarrierType::Empty
    };

    spawn_carrier(commands, position, kind, ids.take(), heading);
}

/// Crea un carrier con identita' assegnata. La riproduzione di una registrazione
/// passa di qui: i carrier devono tornare con lo stesso numero, non con uno nuovo.
/// Il campione lo ha solo chi porta un tubo: e' il tubo il campione.
pub fn spawn_carrier(
    commands: &mut Commands,
    position: Vec3,
    kind: CarrierType,
    carrier_id: u32,
    heading: Heading,
) -> Entity {
    let with_tube = kind == CarrierType::WithTube;
    let sample_id = with_tube
        .then(|| placeholder_sample_id(carrier_id))
        .flatten();

    let mut carrier = commands.spawn((
        Transform::from_translation(position),
        Carrier {
            kind,
            carrier_id,
            sample_id,
            motion: Motion::Straight(heading),
        },
    ));

    if with_tube {
        carrier.with_child(Tube);
    }

    carrier.id()
}

/// Segnaposto in attesa che i campioni arrivino da fuori: e' derivato dal
/// carrier_id solo per avere qualcosa di stabile da guardare a schermo.
fn placeholder_sample_id(carrier_id: u32) -> Option<SampleId> {
    SampleId::new(&format!("SMP-{carrier_id:08}"))
}

/// Quello che il mondo puo' dire a un carrier in movimento.
pub struct Track<'a> {
    pub diverts: &'a [(&'a Divert, Switch, Heading, Vec3)],
    pub turners: &'a [(&'a Turner, Switch, Heading, Vec3)],
    pub reversers: &'a [(&'a Reverser, Switch, Vec3)],
}

/// Spostamento del carrier in questo frame e come si muovera' dopo. L'ordine di
/// precedenza e' quello scritto: chi e' in curva la finisce, poi contano gli
/// oggetti che cambiano marcia, infine la deviazione di corsia.
fn carrier_step(
    carrier: &Carrier,
    translation: Vec3,
    track: &Track,
    delta_secs: f32,
) -> (Vec3, Motion) {
    // 1. Una curva iniziata va portata a termine: finche' dura, il resto tace.
    if let Motion::Turning {
        centre,
        remaining,
        exit,
    } = carrier.motion
    {
        let turned = (BELT_SPEED / TURN_RADIUS * delta_secs).min(remaining);
        let offset = Vec2::from_angle(turned).rotate(translation.truncate() - centre);
        let step = (centre + offset - translation.truncate()).extend(0.0);

        let left = remaining - turned;
        let next = if left <= 0.0 {
            Motion::Straight(exit)
        } else {
            Motion::Turning {
                centre,
                remaining: left,
                exit,
            }
        };

        return (step, next);
    }

    let heading = carrier.motion.heading().unwrap_or(Heading::Left);
    let straight = (heading.as_vec() * BELT_SPEED * delta_secs).extend(0.0);

    // 2. L'inversione vale per qualunque marcia, orizzontale o verticale: la
    //    curva si costruisce rispetto alla direzione del carrier. Dopo il mezzo
    //    giro il carrier esce a una cella di distanza, quindi lo stesso oggetto
    //    non lo riprende e non si crea un cerchio infinito.
    for (reverser, switch, position) in track.reversers {
        if switch.working() && crosses(*position, translation, straight, heading) {
            // Si parte dal centro esatto dell'oggetto: e' quello che rende il
            // diametro della curva una cella piena.
            let snap = (position.truncate() - translation.truncate()).extend(0.0);
            return (snap, reverser.turn(*position, heading));
        }
    }

    // 3. La svolta vale per tutti i carrier, tubo o non tubo: la nuova marcia e'
    //    quella indicata dalla freccia dell'oggetto.
    for (_turner, switch, facing, position) in track.turners {
        if switch.working() && crosses(*position, translation, straight, heading) {
            // Si riparte dal centro esatto dell'oggetto, altrimenti la nuova
            // marcia partirebbe sfalsata rispetto alla griglia.
            let snap = (position.truncate() - translation.truncate()).extend(0.0);
            return (snap, Motion::Straight(*facing));
        }
    }

    // 4. La deviazione di corsia, che vale per chiunque: vuoti e pieni. Chi
    //    debba essere deviato e chi no lo decidera' la logica di impianto
    //    quando ci sara' mqtt; qui c'e' solo il movimento, e il movimento non
    //    guarda dentro al carrier.
    for (divert, switch, facing, position) in track.diverts {
        if !divert.catches(*switch, *position, translation, *facing, heading) {
            continue;
        }

        // La freccia disegna la diagonale: lo spostamento e' la sua componente
        // trasversale alla marcia del carrier.
        let Some(sideways) = Divert::shift(*facing, heading).map(Heading::as_vec) else {
            continue;
        };
        let offset = (translation.truncate() - position.truncate()).dot(sideways);
        // Il passo di lato non supera mai la destinazione: e' questo che fa
        // arrivare il carrier esattamente sulla linea voluta, invece di
        // lasciarlo dove capita quando esce dalla finestra del deviatore.
        let reach = BELT_SPEED * delta_secs;
        let shift = (LANE_HEIGHT - offset).clamp(-reach, reach);
        // Gia' a destinazione: questo deviatore non ha niente da fare, ma un
        // altro sulla stessa linea potrebbe averne.
        if shift == 0.0 {
            continue;
        }

        let forward = heading.as_vec() * CARRIER_DIVERT_SPEED * delta_secs;
        return ((forward + sideways * shift).extend(0.0), carrier.motion);
    }

    (straight, carrier.motion)
}

/// Un ostacolo fermo sulla scena: un rettangolo che i carrier non attraversano.
/// Il gate ne occupa una striscia di lato, un ATR spento tutta la sua cella.
#[derive(Clone, Copy, Debug)]
pub struct Blocker {
    pub centre: Vec3,
    pub half: Vec2,
}

/// Vero se un cerchio di raggio `radius` centrato in `point` tocca l'ostacolo.
/// Il raggio arriva da fuori: cosi' l'ostacolo non ha bisogno di sapere nulla
/// di com'e' fatto un carrier.
pub fn blocks(blocker: Blocker, point: Vec3, radius: f32) -> bool {
    circle_touches_box(blocker.centre, blocker.half, point, radius)
}

/// Quello che serve sapere di un carrier per far avanzare un frame.
pub struct Moving<'a> {
    pub carrier: &'a Carrier,
    pub position: Vec3,
}

/// Fa avanzare tutti i carrier di un frame. Per ciascuno restituisce dove
/// finisce e come si muovera'; `None` se e' stato fermato, perche' chi non
/// avanza non cambia nemmeno stato di moto.
pub fn resolve_frame(
    belt: &[Moving],
    track: &Track,
    blockers: &[Blocker],
    delta_secs: f32,
) -> Vec<(Vec3, Option<Motion>)> {
    let steps: Vec<(Vec3, Motion)> = belt
        .iter()
        .map(|moving| carrier_step(moving.carrier, moving.position, track, delta_secs))
        .collect();

    belt.iter()
        .enumerate()
        .map(|(index, moving)| {
            let (step, motion) = steps[index];
            let candidate = moving.position + step;

            // Il confronto e' con le posizioni attuali di tutti gli altri, non
            // con quelle gia' risolte: da quando i carrier viaggiano anche in
            // verticale e verso destra non esiste un ordine "chi e' davanti",
            // e farlo dipendere da uno sbagliato li faceva incastrare.
            let queued = belt.iter().enumerate().any(|(other, ahead)| {
                if other == index {
                    return false;
                }

                let gap = candidate.distance(ahead.position);
                gap < CARRIER_SIZE && gap < moving.position.distance(ahead.position)
            });

            // Chi sbarra la strada ferma il carrier subito prima di toccarlo.
            // Chi lo stava gia' attraversando quando la strada si e' chiusa
            // finisce il transito invece di restare incastrato dentro l'ostacolo.
            let stopped = blockers.iter().any(|blocker| {
                blocks(*blocker, candidate, CARRIER_RADIUS)
                    && !blocks(*blocker, moving.position, CARRIER_RADIUS)
            });

            if queued || stopped {
                (moving.position, None)
            } else {
                (candidate, Some(motion))
            }
        })
        .collect()
}

fn move_carrier(
    time: Res<Time>,
    mut query: Query<(Entity, &mut Carrier, &mut Transform)>,
    gates: Query<(&Gate, &Switch, &Facing, &Transform), Without<Carrier>>,
    diverts: Query<(&Divert, &Switch, &Facing, &Transform), Without<Carrier>>,
    turners: Query<(&Turner, &Switch, &Facing, &Transform), Without<Carrier>>,
    reversers: Query<(&Reverser, &Switch, &Transform), Without<Carrier>>,
) {
    let delta_secs = time.delta_secs();

    let diverts: Vec<(&Divert, Switch, Heading, Vec3)> = diverts
        .iter()
        .map(|(divert, switch, facing, at)| (divert, *switch, facing.0, at.translation))
        .collect();
    let turners: Vec<(&Turner, Switch, Heading, Vec3)> = turners
        .iter()
        .map(|(turner, switch, facing, at)| (turner, *switch, facing.0, at.translation))
        .collect();
    let reversers: Vec<(&Reverser, Switch, Vec3)> = reversers
        .iter()
        .map(|(reverser, switch, at)| (reverser, *switch, at.translation))
        .collect();
    let track = Track {
        diverts: &diverts,
        turners: &turners,
        reversers: &reversers,
    };

    // Un gate acceso e un ATR spento fanno la stessa cosa al flusso: lo
    // fermano. Per il movimento sono la stessa lista.
    let blockers: Vec<Blocker> = gates
        .iter()
        .filter(|(_, switch, _, _)| switch.working())
        .map(|(_, _, facing, transform)| gate::bar(transform.translation, facing.0))
        // L'ATR spento chiude tutta la sua cella, non un lato: chi arriva non ha
        // nessun modo di passare, ne' dritto ne' di lato.
        .chain(
            diverts
                .iter()
                .filter(|(divert, switch, _, _)| divert.is_blocking(*switch))
                .map(|(_, _, _, position)| Blocker {
                    centre: *position,
                    half: Vec2::splat(PIECE_SIZE / 2.0),
                }),
        )
        .collect();

    let entities: Vec<Entity> = query.iter().map(|(entity, _, _)| entity).collect();
    let positions: Vec<Vec3> = query
        .iter()
        .map(|(_, _, transform)| transform.translation)
        .collect();
    let belt: Vec<Moving> = query
        .iter()
        .zip(&positions)
        .map(|((_, carrier, _), position)| Moving {
            carrier,
            position: *position,
        })
        .collect();

    let resolved = resolve_frame(&belt, &track, &blockers, delta_secs);
    drop(belt);

    for (entity, (translation, motion)) in entities.into_iter().zip(resolved) {
        if let Ok((_, mut carrier, mut transform)) = query.get_mut(entity) {
            transform.translation = translation;

            if let Some(motion) = motion
                && carrier.motion != motion
            {
                carrier.motion = motion;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::divert::{DIVERT_ZONE_HALF_WIDTH, DivertKind, LANE_HEIGHT};
    use crate::gate;
    use crate::grid::GRID_STEP;
    use crate::piece::{ANTENNA_OFFSET, ANTENNA_RADIUS};

    const DELTA: f32 = 1.0 / 60.0;

    /// La corsia principale su cui stanno le sorgenti, in coordinate mondo.
    const MAIN_LANE: f32 = -120.0;

    fn atr() -> Divert {
        Divert {
            kind: DivertKind::Atr,
            engaged: false,
        }
    }

    fn carrier(kind: CarrierType) -> Carrier {
        Carrier {
            kind,
            carrier_id: 1,
            sample_id: None,
            motion: Motion::Straight(Heading::Left),
        }
    }

    /// Un ostacolo che chiude tutta la cella: e' l'ATR spento.
    fn cell_blocker(position: Vec3) -> Blocker {
        Blocker {
            centre: position,
            half: Vec2::splat(PIECE_SIZE / 2.0),
        }
    }

    /// Percorso con i soli deviatori: la maggior parte dei test non ha bisogno
    /// degli altri oggetti.
    /// Un oggetto in servizio e comandato, e uno fuori servizio: nei test
    /// servono di continuo.
    const ON: Switch = Switch {
        enabled: true,
        active: true,
    };
    const OFF: Switch = Switch {
        enabled: false,
        active: true,
    };
    /// In servizio ma non comandato: e' il divert che lascia proseguire dritto
    /// e l'ATR che invece sbarra la strada.
    const OFF_COMMAND: Switch = Switch {
        enabled: true,
        active: false,
    };

    fn only_diverts<'a>(diverts: &'a [(&'a Divert, Switch, Heading, Vec3)]) -> Track<'a> {
        Track {
            diverts,
            turners: &[],
            reversers: &[],
        }
    }

    /// Fa avanzare il carrier finche' non si esaurisce il numero di passi,
    /// aggiornando anche il suo stato di moto.
    fn run(carrier: &mut Carrier, from: Vec3, track: &Track, steps: usize) -> Vec3 {
        let mut position = from;

        for _ in 0..steps {
            let (step, motion) = carrier_step(carrier, position, track, DELTA);
            position += step;
            carrier.motion = motion;
        }

        position
    }

    /// Il caso completo: divert sulla corsia principale, ATR una cella piu' su.
    /// Il carrier deve salire di una corsia esatta e tornare esattamente da dove
    /// era partito, senza portarsi dietro nessuna informazione.
    #[test]
    fn divert_and_atr_hand_the_carrier_back_to_the_main_lane() {
        let divert = Divert {
            kind: DivertKind::Divert,
            engaged: false,
        };
        let deviators = [
            (&divert, ON, Heading::Up, Vec3::new(0.0, MAIN_LANE, 0.0)),
            (
                &atr(),
                ON,
                Heading::Left,
                Vec3::new(-3.0 * GRID_STEP, MAIN_LANE + GRID_STEP, 0.0),
            ),
        ];

        let carrier = carrier(CarrierType::WithTube);
        let mut position = Vec3::new(4.0 * GRID_STEP, MAIN_LANE, 0.0);
        let mut highest = MAIN_LANE;

        for _ in 0..1000 {
            position += carrier_step(&carrier, position, &only_diverts(&deviators), DELTA).0;
            highest = highest.max(position.y);
        }

        assert_eq!(
            highest,
            MAIN_LANE + LANE_HEIGHT,
            "il divert alza di una corsia esatta"
        );
        assert_eq!(
            position.y, MAIN_LANE,
            "l'ATR riporta il carrier sulla corsia principale"
        );
    }

    /// L'ATR da solo: scende di una corsia rispetto a dove e' piazzato.
    #[test]
    fn atr_lowers_the_carrier_by_exactly_one_lane() {
        let atr = atr();
        let atr_position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
        let carrier = carrier(CarrierType::WithTube);
        let mut position = Vec3::new(DIVERT_ZONE_HALF_WIDTH, MAIN_LANE + LANE_HEIGHT, 0.0);

        for _ in 0..200 {
            position += carrier_step(
                &carrier,
                position,
                &only_diverts(&[(&atr, ON, Heading::Left, atr_position)]),
                DELTA,
            )
            .0;
        }

        assert_eq!(position.y, MAIN_LANE);
    }

    /// Il passo verticale si accorcia sull'ultimo tratto invece di scavalcare la quota.
    #[test]
    fn the_last_step_does_not_overshoot() {
        let atr = atr();
        let carrier = carrier(CarrierType::WithTube);
        let almost_there = Vec3::new(0.0, MAIN_LANE + 1.0, 0.0);

        let atr_position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
        let (step, _) = carrier_step(
            &carrier,
            almost_there,
            &only_diverts(&[(&atr, ON, Heading::Left, atr_position)]),
            DELTA,
        );

        assert_eq!(step.y, -1.0, "scende solo il px che manca");
        assert!(
            BELT_SPEED * DELTA > 1.0,
            "senza il limite avrebbe scavalcato la quota"
        );
    }

    /// Un divert spento non deve deviare niente, nemmeno con un ATR acceso poco
    /// piu' avanti che potrebbe agganciare lo stesso carrier.
    #[test]
    fn a_switched_off_divert_leaves_the_flow_alone() {
        let off_divert = Divert {
            kind: DivertKind::Divert,
            engaged: false,
        };
        let live_atr = atr();
        let carrier = carrier(CarrierType::WithTube);
        let deviators = [
            (
                &off_divert,
                OFF_COMMAND,
                Heading::Up,
                Vec3::new(0.0, MAIN_LANE, 0.0),
            ),
            (
                &live_atr,
                ON,
                Heading::Left,
                Vec3::new(-2.0 * GRID_STEP, MAIN_LANE + GRID_STEP, 0.0),
            ),
        ];

        let mut position = Vec3::new(200.0, MAIN_LANE, 0.0);
        for _ in 0..400 {
            position += carrier_step(&carrier, position, &only_diverts(&deviators), DELTA).0;
            assert_eq!(
                position.y, MAIN_LANE,
                "il carrier non deve lasciare la corsia a x = {}",
                position.x
            );
        }
    }

    /// Il caso che mancava: su un flusso verticale il divert sposta il carrier
    /// di una colonna a destra, non di una riga in alto.
    #[test]
    fn a_divert_shifts_a_rising_carrier_one_column_to_its_right() {
        let divert = Divert {
            kind: DivertKind::Divert,
            engaged: false,
        };
        let divert_position = Vec3::ZERO;
        let diverts = [(&divert, ON, Heading::Right, divert_position)];
        let track = only_diverts(&diverts);

        let mut carrier = carrier(CarrierType::WithTube);
        carrier.motion = Motion::Straight(Heading::Up);
        let end = run(&mut carrier, Vec3::new(0.0, -GRID_STEP, 0.0), &track, 400);

        assert!(
            (end.x - GRID_STEP).abs() < 0.01,
            "deve trovarsi una colonna a destra, non a x = {}",
            end.x
        );
        assert_eq!(
            carrier.motion,
            Motion::Straight(Heading::Up),
            "la marcia non cambia: il divert sposta di lato, non gira"
        );
    }

    /// L'ATR e' lo specchio del divert e segue la stessa regola relativa: su un
    /// flusso verticale sposta di una colonna a sinistra del carrier.
    #[test]
    fn an_atr_shifts_a_rising_carrier_one_column_to_its_left() {
        let atr_position = Vec3::ZERO;
        let diverts = [(&atr(), ON, Heading::Up, atr_position)];
        let track = only_diverts(&diverts);

        let mut carrier = carrier(CarrierType::WithTube);
        carrier.motion = Motion::Straight(Heading::Up);
        let end = run(&mut carrier, Vec3::new(0.0, -GRID_STEP, 0.0), &track, 400);

        assert!(
            (end.x - (-GRID_STEP)).abs() < 0.01,
            "deve trovarsi una colonna a sinistra, non a x = {}",
            end.x
        );
        assert_eq!(carrier.motion, Motion::Straight(Heading::Up));
    }

    /// Divert e ATR si annullano a vicenda su qualunque marcia, non solo
    /// sull'orizzontale: e' quello che li rende una coppia.
    #[test]
    fn divert_and_atr_cancel_out_on_a_vertical_flow() {
        let divert = Divert {
            kind: DivertKind::Divert,
            engaged: false,
        };
        let diverts = [
            (&divert, ON, Heading::Right, Vec3::new(0.0, 0.0, 0.0)),
            (
                &atr(),
                ON,
                Heading::Up,
                Vec3::new(GRID_STEP, 3.0 * GRID_STEP, 0.0),
            ),
        ];
        let track = only_diverts(&diverts);

        let mut carrier = carrier(CarrierType::WithTube);
        carrier.motion = Motion::Straight(Heading::Up);
        let end = run(&mut carrier, Vec3::new(0.0, -GRID_STEP, 0.0), &track, 800);

        assert!(
            end.x.abs() < 0.01,
            "deve essere tornato sulla colonna di partenza, non a x = {}",
            end.x
        );
    }

    /// Chi va a sinistra svolta verso l'alto: la destra e' quella del carrier.
    #[test]
    fn a_turner_sends_a_leftward_carrier_upwards() {
        let turner = Turner { engaged: false };
        let turner_position = Vec3::new(0.0, MAIN_LANE, 0.0);
        let track = Track {
            diverts: &[],
            turners: &[(&turner, ON, Heading::Up, turner_position)],
            reversers: &[],
        };

        let mut carrier = carrier(CarrierType::Empty);
        let start = Vec3::new(2.0 * GRID_STEP, MAIN_LANE, 0.0);
        let end = run(&mut carrier, start, &track, 400);

        assert_eq!(carrier.motion, Motion::Straight(Heading::Up));
        assert_eq!(end.x, turner_position.x, "sale sulla colonna dell'oggetto");
        assert!(
            end.y > MAIN_LANE + 2.0 * GRID_STEP,
            "ha continuato a salire"
        );
    }

    /// Il caso dello screenshot: flusso che sale, divert che lo sposta di una
    /// colonna a destra e ATR che ce lo riporta. La freccia dell'ATR punta a
    /// sinistra, ed e' li' che il carrier deve andare.
    #[test]
    fn an_atr_brings_a_rising_flow_back_to_the_divert_column() {
        let divert = Divert {
            kind: DivertKind::Divert,
            engaged: false,
        };
        let diverts = [
            (&divert, ON, Heading::Right, Vec3::new(0.0, 0.0, 0.0)),
            (
                &atr(),
                ON,
                Heading::Up,
                Vec3::new(GRID_STEP, 4.0 * GRID_STEP, 0.0),
            ),
        ];
        let track = only_diverts(&diverts);

        let mut carrier = carrier(CarrierType::WithTube);
        carrier.motion = Motion::Straight(Heading::Up);
        let end = run(&mut carrier, Vec3::new(0.0, -GRID_STEP, 0.0), &track, 900);

        assert!(
            end.x.abs() < 0.01,
            "l'ATR deve riportarlo sulla colonna del divert, non a x = {}",
            end.x
        );
        assert!(end.y > 4.0 * GRID_STEP, "e deve aver continuato a salire");
    }

    /// Il caso dello screenshot: l'ATR viene spento mentre un carrier e' a meta'
    /// manovra. Quel carrier deve finire di rientrare, non proseguire dritto
    /// restando fra due corsie.
    #[test]
    fn a_carrier_caught_mid_manoeuvre_still_finishes_it() {
        let off_atr = Divert {
            kind: DivertKind::Atr,
            engaged: false,
        };
        let atr_position = Vec3::ZERO;
        let diverts = [(&off_atr, OFF_COMMAND, Heading::Up, atr_position)];
        let track = only_diverts(&diverts);

        let carrier = carrier(CarrierType::WithTube);
        // Gia' dentro la manovra: mezza corsia sopra la linea di partenza.
        let start = Vec3::new(0.0, LANE_HEIGHT / 2.0, 0.0);

        let mut position = start;
        for _ in 0..600 {
            let resolved = {
                let belt = [Moving {
                    carrier: &carrier,
                    position,
                }];

                resolve_frame(&belt, &track, &[cell_blocker(atr_position)], DELTA)
            };
            position = resolved[0].0;
        }

        assert!(
            (position.y - LANE_HEIGHT).abs() < 0.01,
            "doveva completare il rientro fino alla corsia, invece e' a y = {}",
            position.y
        );
    }

    /// Il motivo per cui il gate e' una sbarra e non un quadrato: il carrier che
    /// ferma deve restare sopra l'antenna che sta al centro di quella cella, e
    /// il successivo deve restarne fuori. Con il gate quadrato di prima si
    /// fermavano entrambi mezza cella piu' in la', e a stare sull'antenna era il
    /// secondo.
    #[test]
    fn the_carrier_a_gate_stops_comes_to_rest_over_the_antenna() {
        let empty_track = Track {
            diverts: &[],
            turners: &[],
            reversers: &[],
        };
        let cell = Vec3::ZERO;
        let blockers = [gate::bar(cell, Heading::Left)];

        let carrier = carrier(CarrierType::WithTube);
        let mut positions = [
            Vec3::new(3.0 * GRID_STEP, 0.0, 0.0),
            Vec3::new(4.0 * GRID_STEP, 0.0, 0.0),
        ];

        for _ in 0..900 {
            let resolved = {
                let belt: Vec<Moving> = positions
                    .iter()
                    .map(|position| Moving {
                        carrier: &carrier,
                        position: *position,
                    })
                    .collect();

                resolve_frame(&belt, &empty_track, &blockers, DELTA)
            };
            for (position, (resolved, _)) in positions.iter_mut().zip(resolved) {
                *position = resolved;
            }
        }

        // L'antenna della stessa cella, girata come il gate: sta dove il
        // carrier si ferma, non al centro della cella.
        let antenna = cell + (Heading::Left.as_vec() * ANTENNA_OFFSET).extend(0.0);
        let first = positions[0].distance(antenna);
        let second = positions[1].distance(antenna);

        assert!(
            first < 2.0,
            "il carrier fermato deve stare sopra l'antenna: dista {first}"
        );
        assert!(
            second > ANTENNA_RADIUS,
            "quello dietro no, altrimenti non si saprebbe chi e' stato letto: dista {second}"
        );
    }

    /// Un ATR spento non apre una via dritta: il carrier gli si ferma davanti
    /// invece di proseguire, e li' resta finche' non lo si riaccende.
    #[test]
    fn a_switched_off_atr_stops_the_flow() {
        let empty_track = Track {
            diverts: &[],
            turners: &[],
            reversers: &[],
        };
        let atr = Vec3::ZERO;

        let carrier = carrier(CarrierType::WithTube);
        let mut position = Vec3::new(3.0 * GRID_STEP, 0.0, 0.0);

        for _ in 0..600 {
            let resolved = {
                let belt = [Moving {
                    carrier: &carrier,
                    position,
                }];

                resolve_frame(&belt, &empty_track, &[cell_blocker(atr)], DELTA)
            };
            position = resolved[0].0;
        }

        assert!(
            position.x > 0.0,
            "il carrier deve fermarsi davanti all'ATR, non attraversarlo: x = {}",
            position.x
        );
        assert!(
            position.x < CARRIER_SIZE,
            "e deve arrivarci vicino, non fermarsi a meta' strada: x = {}",
            position.x
        );
    }

    /// Una fila di carrier deve attraversare la svolta senza incastrarsi: il
    /// primo che gira cambia direzione e gli altri devono potergli andare
    /// dietro, non fermarsi contro di lui.
    #[test]
    fn a_queue_of_carriers_flows_through_a_turner() {
        let turner = Turner { engaged: false };
        let turners = [(&turner, ON, Heading::Up, Vec3::ZERO)];
        let track = Track {
            diverts: &[],
            turners: &turners,
            reversers: &[],
        };

        let mut carriers: Vec<Carrier> = (0..3).map(|_| carrier(CarrierType::Empty)).collect();
        // Distanziati come li emette una sorgente: mezzo secondo l'uno dall'altro.
        let mut positions: Vec<Vec3> = (0..3)
            .map(|index| Vec3::new((index as f32 + 1.0) * 50.0, 0.0, 0.0))
            .collect();

        for _ in 0..900 {
            let resolved = {
                let belt: Vec<Moving> = carriers
                    .iter()
                    .zip(&positions)
                    .map(|(carrier, position)| Moving {
                        carrier,
                        position: *position,
                    })
                    .collect();

                resolve_frame(&belt, &track, &[], DELTA)
            };

            for (index, (position, motion)) in resolved.into_iter().enumerate() {
                positions[index] = position;
                if let Some(motion) = motion {
                    carriers[index].motion = motion;
                }
            }
        }

        for (index, carrier) in carriers.iter().enumerate() {
            assert_eq!(
                carrier.motion,
                Motion::Straight(Heading::Up),
                "il carrier {index} non ha girato"
            );
            assert!(
                positions[index].y > 2.0 * GRID_STEP,
                "il carrier {index} e' rimasto fermo a y = {}",
                positions[index].y
            );
        }
    }

    /// La freccia decide la nuova marcia, non la vecchia: lo stesso oggetto
    /// girato verso destra manda a destra chiunque ci passi.
    #[test]
    fn the_arrow_decides_the_new_heading() {
        let turner = Turner { engaged: false };
        let turner_position = Vec3::ZERO;
        let track = Track {
            diverts: &[],
            turners: &[(&turner, ON, Heading::Right, turner_position)],
            reversers: &[],
        };

        let mut carrier = carrier(CarrierType::Empty);
        carrier.motion = Motion::Straight(Heading::Up);
        let end = run(
            &mut carrier,
            Vec3::new(0.0, -2.0 * GRID_STEP, 0.0),
            &track,
            400,
        );

        assert_eq!(carrier.motion, Motion::Straight(Heading::Right));
        assert_eq!(end.y, turner_position.y, "prosegue sulla riga dell'oggetto");
        assert!(end.x > 2.0 * GRID_STEP, "e' andato verso destra");
    }

    /// Quattro svolte a destra riportano il carrier nella marcia iniziale.
    #[test]
    fn four_right_turns_come_back_to_the_start() {
        let heading = Heading::Left;

        assert_eq!(
            heading.turn_right().turn_right().turn_right().turn_right(),
            heading
        );
    }

    #[test]
    fn a_switched_off_turner_lets_the_carrier_through() {
        let turner = Turner { engaged: false };
        let track = Track {
            diverts: &[],
            turners: &[(&turner, OFF, Heading::Up, Vec3::new(0.0, MAIN_LANE, 0.0))],
            reversers: &[],
        };

        let mut carrier = carrier(CarrierType::Empty);
        let end = run(
            &mut carrier,
            Vec3::new(GRID_STEP, MAIN_LANE, 0.0),
            &track,
            200,
        );

        assert_eq!(carrier.motion, Motion::Straight(Heading::Left));
        assert_eq!(end.y, MAIN_LANE);
    }

    /// Il punto dell'inversione: il carrier riparte verso destra su una linea
    /// diversa da quella di andata, senza mai risalire sopra di essa.
    #[test]
    fn the_reverser_sends_the_carrier_back_on_another_line() {
        let reverser = Reverser { engaged: false };
        let reverser_position = Vec3::new(0.0, MAIN_LANE, 0.0);
        let track = Track {
            diverts: &[],
            turners: &[],
            reversers: &[(&reverser, ON, reverser_position)],
        };

        let mut carrier = carrier(CarrierType::Empty);
        let mut position = Vec3::new(2.0 * GRID_STEP, MAIN_LANE, 0.0);
        let mut highest: f32 = MAIN_LANE;

        for _ in 0..400 {
            let (step, motion) = carrier_step(&carrier, position, &track, DELTA);
            position += step;
            carrier.motion = motion;
            highest = highest.max(position.y);
        }

        assert_eq!(carrier.motion, Motion::Straight(Heading::Right));
        assert!(
            (position.y - (MAIN_LANE - GRID_STEP)).abs() < 0.01,
            "rientra una cella sotto, non a {}",
            position.y
        );
        assert!(
            highest <= MAIN_LANE + 0.01,
            "non deve mai risalire sopra la linea di andata"
        );
        assert!(position.x > 0.0, "e' ripartito verso destra");
    }

    /// Il caso che mancava: un carrier che sale viene invertito e rimandato giu'
    /// su una colonna diversa da quella di salita.
    #[test]
    fn the_reverser_also_turns_a_rising_carrier() {
        let reverser = Reverser { engaged: false };
        let reverser_position = Vec3::new(0.0, 0.0, 0.0);
        let track = Track {
            diverts: &[],
            turners: &[],
            reversers: &[(&reverser, ON, reverser_position)],
        };

        let mut carrier = carrier(CarrierType::Empty);
        carrier.motion = Motion::Straight(Heading::Up);

        let start = Vec3::new(0.0, -2.0 * GRID_STEP, 0.0);
        let end = run(&mut carrier, start, &track, 400);

        assert_eq!(carrier.motion, Motion::Straight(Heading::Down));
        assert!(
            (end.x - (-GRID_STEP)).abs() < 0.01,
            "deve scendere una colonna di lato, non a x = {}",
            end.x
        );
        assert!(end.y < 0.0, "sta scendendo");
    }

    #[test]
    fn a_switched_off_reverser_lets_the_carrier_through() {
        let reverser = Reverser { engaged: false };
        let track = Track {
            diverts: &[],
            turners: &[],
            reversers: &[(&reverser, OFF, Vec3::new(0.0, MAIN_LANE, 0.0))],
        };

        let mut carrier = carrier(CarrierType::Empty);
        let end = run(
            &mut carrier,
            Vec3::new(GRID_STEP, MAIN_LANE, 0.0),
            &track,
            200,
        );

        assert_eq!(carrier.motion, Motion::Straight(Heading::Left));
        assert!(end.x < -GRID_STEP, "ha tirato dritto");
    }

    #[test]
    fn the_counter_hands_out_one_id_per_carrier() {
        let mut ids = NextCarrierId::default();

        assert_eq!(ids.take(), 1, "si parte da 1, non da 0");
        assert_eq!(ids.take(), 2);
        assert_eq!(ids.take(), 3);
    }

    #[test]
    fn a_sample_id_longer_than_the_limit_cannot_exist() {
        let limit = "x".repeat(SAMPLE_ID_MAX_LEN);
        let too_long = "x".repeat(SAMPLE_ID_MAX_LEN + 1);

        assert!(SampleId::new(&limit).is_some(), "24 caratteri sono ammessi");
        assert!(SampleId::new(&too_long).is_none());
        assert!(SampleId::new("").is_none(), "un id vuoto e' assenza di id");
    }

    /// Il limite e' sui caratteri: contare i byte scarterebbe id legittimi.
    #[test]
    fn accented_characters_count_as_one() {
        let accented = "à".repeat(SAMPLE_ID_MAX_LEN);

        assert_eq!(accented.len(), SAMPLE_ID_MAX_LEN * 2, "sono 48 byte");
        assert!(SampleId::new(&accented).is_some());
    }

    #[test]
    fn only_carriers_with_a_tube_carry_a_sample() {
        assert!(placeholder_sample_id(42).is_some());
        assert_eq!(
            placeholder_sample_id(42).unwrap().as_str(),
            "SMP-00000042",
            "sta comodamente nei 24 caratteri"
        );
    }

    #[test]
    fn every_carrier_is_diverted_whatever_it_carries() {
        for kind in [CarrierType::Empty, CarrierType::WithTube] {
            let carrier = carrier(kind);
            let position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
            let (step, _) = carrier_step(
                &carrier,
                position,
                &only_diverts(&[(&atr(), ON, Heading::Left, position)]),
                DELTA,
            );

            assert!(
                step.y < 0.0,
                "il movimento non guarda dentro al carrier: {kind:?} deve scendere"
            );
        }
    }
}
