# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Simulatore 2D di un impianto a carrier (nastro, deviatori, gate, sensori), scritto in Rust con Bevy 0.18.
Un solo binario, `chapter1`. Prosa, commenti e interfaccia sono in italiano.

## Comandi

```bash
cargo build
cargo run -- --layout layout.ron          # con finestra
cargo run -- --hide_gui --layout x.ron    # senza finestra, fino a Ctrl+C
cargo run -- --layout x.ron --record      # registra da subito (anche headless)
cargo run -- --replay registrazione-*.ron # riproduce; vietato con --hide_gui

cargo test                                 # ~105 test, girano in millisecondi
cargo test divert                          # per sottostringa (modulo o nome del test)
cargo test divert::tests::the_catch_band_covers_the_manoeuvre_only -- --exact  # uno solo
cargo fmt                                  # il repo e' rustfmt-clean: tenerlo cosi'
cargo clippy
```

`cargo clippy` non e' a zero warning: ne restano ~28 preesistenti (tipi complessi e troppi
argomenti, cioe' le firme dei sistemi Bevy). Non partire a ripulirli: verifica solo di non
averne aggiunti sui file toccati, con `cargo clippy 2>&1 | grep <file>`.

Git traccia solo `src/` e i file di cargo: `.gitignore` esclude tutti i `*.ron`, quindi
`layout.ron` e le registrazioni sono roba locale e non vanno committate. Un clone pulito
parte senza layout, e senza `--layout` la scena nasce vuota.

`../chapter1` e' la versione originale del libro; questa e' la copia modificata.

## Architettura

### La regola che spiega la forma del codice

Tutto deve poter girare **senza finestra** (`--hide_gui`), perche' la simulazione dovra'
parlare con un programma di comando via mqtt. Da qui la divisione che si ritrova ovunque:
ogni oggetto ha un `XPlugin` (comportamento, montato sempre) e un `XVisualsPlugin`
(mesh, materiali, colori, montato solo con la finestra) — vedi `main.rs`, che monta i due
gruppi separatamente. La logica non deve mai toccare mesh, materiali o camera.
Aggiungendo un oggetto nuovo, si copia quello schema.

### Stati

Due assi indipendenti, entrambi `States` di Bevy:

- `editor::Mode` — `Editing` / `Simulating`: due mestieri con gli stessi tasti del mouse.
- `simulation::SimulationState` — `Running` / `Paused` / `Replaying`.

I sistemi si agganciano con `run_if(in_state(...))`. Passando in editor il tempo si ferma e
il nastro si svuota; durante una riproduzione le posizioni arrivano dal file, quindi la
cinematica vera non deve girare.

### Un oggetto piazzato

E' un'entita' con `layout::Placed { tool, cell }` + `piece::Facing` + `switch::Switch` +
`name::PieceId`/`PieceName` + il componente specifico (`Gate`, `Divert`, `Sensor`, …).
I pezzi passivi (`Tool::Guide`) non prendono ne' interruttori ne' identita': sono disegno.

Nascono **solo** da `layout::place_in_cell`, unico punto usato dal clic dell'editor, dal
bottone Carica e dall'avvio da riga di comando. Aggiungere un tipo di oggetto vuol dire
toccare: la variante di `piece::Tool`, il `match` di `place_in_cell`, `piece::dressing`,
la radice del nome in `name::prefix`, e la barra `MODES` in `editor.rs`.

`piece::Tool` e' il vocabolario dei tipi piazzabili **ed e' anche il vocabolario
serializzato** di `layout.ron`: rinominare una variante invalida i file salvati (per questo
`Turner` porta ancora `#[serde(alias = "Riser")]`). `Tool::layer()` decide la quota z, e
regola quali oggetti possono condividere una cella (l'antenna sta sotto, i sensori di lato).

### Movimento dei carrier

Sta in `carrier.rs`, ed e' la parte piu' densa. `move_carrier` e' l'unico sistema ECS:
raccoglie dal World e poi chiama `resolve_frame` → `carrier_step`, che sono **funzioni pure**
(niente Query, niente World). Le regole della cinematica si scrivono e si provano li' —
per questo carrier.rs ha tanto test quanto codice.

Ordine di precedenza dentro `carrier_step`: una curva iniziata va finita, poi le inversioni,
poi le svolte, infine la deviazione di corsia. Il carrier non conosce il percorso: sono gli
oggetti che attraversa a cambiargli il moto, e il confronto e' un **attraversamento del
centro** (`crosses`), non una vicinanza — e' cio' che impedisce a un oggetto di riprendere
il carrier che ha appena girato. `Blocker` unifica quello che ferma il flusso: un gate
comandato e un ATR spento sono la stessa cosa per il movimento.

Geometria: `GRID_STEP == LANE_HEIGHT == 64`. Una corsia e' alta esattamente una cella, che e'
anche di quanto un divert sposta un carrier. Il layout salva indici di cella, non pixel.

### Stato degli oggetti

`Switch` sono due booleani per tutti: `enabled` (in servizio) e `active`. Cambia cosa
significano: `working()` per gli attuatori (fa la sua azione), `forcing()` per i sensori
(dichiara una presenza che non c'e', per collaudare il programma di comando).
`switch::Look` deriva da un solo colore i quattro gradini con cui l'oggetto racconta il
proprio stato: fuori servizio / non comandato / comandato / con un carrier fra le mani.

Il trait `engagement::Engaged` con il sistema generico `mark_engaged::<T>` risponde a "chi ha
in mano chi" per sensori, antenne, deviatori, svolte e inversioni. Gira anche headless: quello
stato e' destinato a mqtt, non solo al colore.

### Layout e registrazioni

`layout.ron` e' l'elenco degli oggetti (`id`, `tool`, `cell`, `facing`, `name`) e **non**
contiene gli interruttori: il file descrive l'impianto, non la sua configurazione del momento.
Ogni campo aggiunto nel tempo ha `#[serde(default)]` perche' i file vecchi continuino ad
aprirsi — regola da mantenere.

Una registrazione (`registrazione-<epoch>.ron`, `trace.rs`) porta con se' il layout, cosi' e'
uno scenario intero. I frame sono codificati a differenze: compaiono solo i carrier che si
sono mossi, gli interruttori solo quando cambiano, e `gone` dice chi e' uscito (senza,
"assente" significherebbe insieme "fermo" e "uscito"). La riproduzione mette da parte il
layout in scena (`ParkedLayout`) e lo rimette al termine.

## Convenzioni

- **Lingua**: commenti, doc e stringhe dell'interfaccia in italiano **senza accenti**
  (`e'`, `piu'`, `cosi'`). Identificatori e nomi dei test in inglese. Messaggi di commit in
  inglese, una riga.
- **I commenti dicono il perche'**, quasi sempre citando il caso concreto che la regola evita
  ("prima si accendevano anche per un carrier che attraversava la cella senza…"). E' il valore
  del repo: se cambi una regola, aggiorna la spiegazione invece di lasciarla mentire.
- **I test sono frasi**: `a_switched_off_atr_bars_the_way_instead_of_opening_it`. Stanno in un
  `mod tests` in fondo al file e provano **regole pure** — funzioni come `catches`, `covers`,
  `resolve_frame`, `next_free`. Le figure disegnate (mesh, frecce, glifi) non si testano: si
  guardano. `engagement.rs` e' l'unico che monta una `App` per far girare un sistema vero.
- **Un modulo per concetto.** I moduli piccoli (`gate`, `turner`, `reverser`, `guide`, …) sono
  tutti componente + `spawn_x` + plugin visuale, 100-200 righe: e' il modello da copiare.
- `mqtt` ricorre nei commenti come il "domani" verso cui il codice e' orientato: id stabili e
  mai riusati, nomi unici e validi come topic, stato `engaged` calcolato nella logica e non
  nella grafica. Le decisioni prese per quel motivo vanno rispettate.
