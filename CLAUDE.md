# Idle WalkingPad 3D

Projekt jest proceduralną grą 3D w Ruście (macroquad 0.4.16). Biegacz porusza
się zgodnie z prędkością bieżni, złoto zachowuje ekonomię 1:1
(4 km/h = 4 gold/min), a HUD, sklep, album i questy są nakładką 2D. Nie ma
zewnętrznych assetów.

Zasada języka (2026-10-17): wszystkie teksty gracza — HUD, sklep, album,
questy, nazwy miejscowości — po angielsku.

## Zasady

- Ekonomia 1:1 (4 km/h = 4 gold/min). Dystans to aktywność na bieżni: na
  postoju i bez świeżego pliku mostka BLE nie ma dystansu ani dochodu.
  Brak zarobku offline i brak liczenia postępu za zamkniętym oknem.
- Pusty legacy cache nigdy nie generuje drogi/miasta.
- Escape i zamknięcie okna zapisują przed wyjściem; tryby diagnostyczne
  nie dotykają zapisu.
- Na polecenie użytkownika: bez testów/benchmarków/Clippy/walidacji shaderów
  i bez automatycznych zrzutów. Weryfikacja robót: przegląd źródeł,
  formatowanie i build release; kompilacja do startu gry pozostaje dozwolona.

## Komendy

```bash
cargo build --offline --locked
cargo build --offline --locked --release
./target/debug/walkingpad-idle                 # normalnie: /tmp/walkingpad-speed
./target/debug/walkingpad-idle --sim 4.2       # normalny zapis + symulowana taśma
./target/debug/walkingpad-idle --scene night --quality high \
  --capture /tmp/walkingpad-night.png --capture-frames 90
./target/debug/walkingpad-idle --scene rain --sim 4.2 \
  --benchmark 6 --quality medium
```

`--scene`, `--capture` i `--benchmark` są trybami diagnostycznymi: tworzą
`Save::default()` wyłącznie w pamięci i nie wykonują `load_save()` ani
`store_save()`, także po Escape. `--sim` bez jednego z tych przełączników jest
normalnym trybem i korzysta z `save.json` w bieżącym katalogu. Błędne sceny,
jakość, liczby, ścieżki i nieznane opcje są zgłaszane przed dostępem do zapisu.
`--sim` przyjmuje wartości 0–20 000 km/h tylko jako bezpieczny zakres
diagnostyczny; odczyt z mostka BLE nie dostaje nowego limitu sprzętowego.
`--3d` i `--2d` pozostają zgodnymi flagami-no-opami i uruchamiajami ten sam
renderer 3D.

Tryby scen: `day`, `dawn`, `night`, `rain`, `fog`, `physics`, `two-lights`
oraz `boundary`. Lampy są wybierane z rzeczywistych, deterministycznych
obiektów generatora (para stabilnych identyfikatorów źródeł jest wypisywana na
stderr), a `boundary` ustawia kamerę na prawdziwej granicy dwóch biomów.
Capture używa stałego kroku symulacji 1/60 s, stałego seedowania efektów i
ustalonej fazy sceny; normalna gra nadal używa czasu rzeczywistego. PNG zawiera
całe okno, w tym HUD.

Benchmark ma sekundę rozgrzewki i mierzy zadany czas. Raport rozdziela
`elapsed_s`/`avg_fps`/percentyle `frame_ms` (czas między klatkami) od
`renderer_cpu_*_ms` (CPU renderera, bez pomiaru GPU) oraz podaje jakość i
rzeczywisty viewport. Do ręcznej weryfikacji obrazu można wykonać:

```bash
cargo build --offline --locked --release
./target/release/walkingpad-idle --scene day --quality high \
  --capture /tmp/walkingpad-day.png --capture-frames 90
./target/release/walkingpad-idle --scene night --quality high \
  --capture /tmp/walkingpad-night.png --capture-frames 90
./target/release/walkingpad-idle --scene two-lights --quality high \
  --capture /tmp/walkingpad-two-lights.png --capture-frames 90
./target/release/walkingpad-idle --scene boundary --quality high \
  --capture /tmp/walkingpad-boundary.png --capture-frames 90
```

Jakość cieni i LOD:

- `low`: mapa kierunkowa 1024², mapy lamp 256², LOD od 95 m;
- `medium` (domyślna): 2048², 512², LOD od 135 m;
- `high`: 4096², 1024², LOD od 180 m.

Rozmiar mapy jest używany zarówno w stabilizacji projekcji, jak i w texelach
PCF. Geometria koloru i map cieni wybiera ten sam LOD; cache statycznego świata
ma ograniczoną liczbę regionów i kafli.

## Architektura i konwencje

- `src/main.rs` — CLI, pętla gry, mostek prędkości, ekonomia, questy, album i
  warunek save-free dla diagnostyki.
- `src/render3d/mod.rs` — kolejność passów 3D, kamera, jakość i benchmark;
  `geometry.rs` buduje siatki, `world.rs` cache'uje deterministyczne regiony,
  `runner.rs` rysuje pozę fizyki, a `sky.rs`/`weather.rs` środowisko.
- `src/render3d/material.rs` — bazowe kolory sRGB, normalne, roughness/emission,
  liniowe światło, fog po oświetleniu i wspólny shader powierzchni.
- `src/render3d/lighting.rs` — wybór do 8 lokalnych źródeł i płynne sloty;
  `shadow.rs` — mapa kierunkowa oraz dwie mapy lamp, PCF, bias i stabilizacja.
  Te passy używają tych samych trójkątów co kolor, bez nakładanych plam światła.
- `src/world.rs`/`src/rng.rs` — pozycje absolutne w metrach, stabilne ID lamp,
  obiektów i odkryć albumu; region ma 300 m.
- `src/physics.rs` — Rapier3D, stały krok 1/60 s, ograniczony catch-up,
  kontroler kapsuły, grawitacja, dynamiczny fixture diagnostyczny i rebasing
  lokalnego początku przy zachowaniu trasy f64.
- `src/daynight.rs` — doba zależna od dystansu (`DAY_M = 2000`), paleta,
  nocność i jasność. `init_offset()` używa sekund epoki modulo 86400, czyli
  UTC; nie zmieniaj tego na lokalną strefę bez zmiany semantyki zapisu.
- `src/save.rs` — stary schemat JSON i ekonomia. Nowe pola muszą mieć
  `#[serde(default)]` oraz normalizację w `load_save()`.

Kamera używa absolutnego `cam_m = total_km * 1000` jako f64, a renderer
przelicza obiekty na lokalny układ przy `x = 0` biegacza. Nie zawijaj tej
pozycji. Światło przechodzi do shaderów jako liniowe RGB, ale kolory bazowe i
UI pozostają rozdzielone. Cienie zachowują światło nieba i innych lamp, a
widoczność źródła skaluje tylko jego wkład.

## Weryfikacja i ograniczenia

```bash
cargo fmt --all -- --check
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
cargo build --offline --locked --release
bash scripts/check-shaders.sh
```

Testy logiki są headless i nie tworzą zasobów GL. Obejmują zapis wsteczny,
ekonomię bez mnożnika, postój/start-stop, 4 i 20 km/h, fizykę przy różnych
FPS, grawitację i kontakt, szybkie przejścia regionów, origin 1e9 m, cache,
ciągłość biomów, ciągłość źródła słońca, projekcje kierunkowe/lamp oraz
pakowanie głębi i kolory materiałów. Shader helper kompiluje osobno GLSL 100;
sam Rust nie jest dowodem kompilacji shaderów.

Weryfikacja nadzoru z 2026-10-09: 83 testy, formatowanie, Clippy z
`-D warnings`, cztery shadery GLSL i build release przechodzą. Na Intel Arc
B580/Mesa 26.2.2 uruchomiono i obejrzano sceny day, dawn, night/two-lights,
rain, fog, physics i boundary. Podłoże nie ma wcześniejszych prążków
samocieniowania, przejścia biomów są interpolowane w siatkach, a światło
latarni zachowuje rozpoznawalne kolory postaci. Upuszczony obiekt spada i
osiada na drodze także przy prędkości 0. Legacy `--2d` również uruchamia 3D.

Release, nocny deszcz, 4,2 km/h, okno 1883×2123, rozgrzewka 1 s + pomiar
6 s: medium 60,0 FPS/p95 17,07 ms/CPU renderera 2,94 ms; high
59,7 FPS/p95 17,34 ms/CPU 3,03 ms. Syntetyczny przelot przez regiony
z lasami i osadami przy 1440 km/h: medium 60,0 FPS, cache 5 regionów i
27 kafli. To krótkie pomiary na tej maszynie, nie gwarancja dla innych GPU
ani profilowanie czasu GPU. Pole `viewport` raportu podaje rozmiar całego
okna; obszar świata jest pomniejszony o panel sklepu.

Wygląd pozostaje proceduralny/low-poly, nie fotorealistyczny. Cienie używają
map i PCF, nie ray tracingu; fizyka ma płaskie podłoże, przybliżone collidery
obiektów i kontroler postaci, nie pełny ragdoll. Deszcz jest lekkim efektem,
nie osobnym ciałem Rapiera dla każdej kropli. Testy diagnostyczne nie zmieniły
aktualnego zapisu użytkownika ani ekonomii.

Ramiona są przyłączone do boków tułowia; test pełnego cyklu chodu dla
0/4,2/20 km/h sprawdza połączenie barków i prześwit kończyn z uwzględnieniem
ich promieni. Parapet miejskich domów ma górną powierzchnię na dolnej
krawędzi okien i korzysta z tej samej fasady od drogi dla obu stron świata.

Animacja rąk utrzymuje długości odcinków 0,35/0,29 m i płynnie zmienia wymach
oraz ugięcie łokci. Ręce i pochylenie tułowia korzystają z wygładzonej
prędkości `RunnerVisual`, bez osobnego zegara renderera. Sam zakres pozy
wizualnej jest ograniczony do 20 km/h, bez ograniczania trasy ani BLE.
Po tej korekcie przeszło 87 testów; późniejsze polecenie użytkownika wyłącza
dalsze testy i benchmarki. Kompilacja potrzebna do startu gry pozostaje
dozwolona. Powyższe pomiary wydajności są wcześniejsze niż ta korekta.

Latarnie mają wysięgnik 0,9 m do środka drogi i słupy 0,5 m poza krawędzią
asfaltu. Wspólny `lamp_head_offset(side)` jest prawdziwym przesunięciem od
słupa: model i źródło dodają je dokładnie raz. Daje to słup na ±3,1 m,
klosz na ±2,2 m i stożek skierowany na środek jezdni. Poprawiono wcześniejsze
podwójne dodanie pozycji bocznej w rendererze. Ta korekta była formatowana
i kompilowana w release, bez nowych testów ani benchmarków na polecenie
użytkownika; wcześniejsze wyniki 83/87 testów nie są jej weryfikacją.

Oprawa latarni jest teraz płaską lampą LED: grafitowa obudowa 0,28×0,64 m,
grubość 0,09 m, ścięte narożniki i wpuszczony panel na spodzie. Środek panelu
pokrywa się ze źródłem światła, a jego normalna z kierunkiem stożka na drogę;
ramię mocuje się z tyłu, obudowa nie przecina osi emisji. Bliski LOD ma
subtelne paski LED, daleki zachowuje płaską sylwetkę. Wspólne białe światło
`LAMP_LED_COLOR_SRGB` zastępuje pomarańczową kulę. Flaga `NIGHT_LED` dodaje
emisję nocą, nie przemalowując panelu ani materiałów okien; w dzień panel
pozostaje nieemisyjny. Po redesignie wykonano formatowanie i build release,
bez testów, benchmarków oraz walidacji shaderów na życzenie użytkownika.

Normalne instancje uruchomione w tym samym katalogu współdzielą `save.json`,
więc dla gry użytkowej należy uruchamiać jedną instancję na katalog zapisu.
Tryby diagnostyczne są save-free i nie mają tego konfliktu (nadal konkurują o
GPU). Domyślny backend macroquad/miniquad w tym środowisku zakłada X11Only —
nie zakładaj backendu Wayland.

Proces zatrzymuj po sprawdzeniu konkretnego PID i binarki, np. `pgrep -x
walkingpad-idle`, potem `readlink /proc/<PID>/exe` i dopiero `kill <PID>`.
Nie używaj szerokiego `pgrep -f ... | ... | kill`; nazwa wykonywalna
`walkingpad-idle` mieści się w limicie nazwy procesu i `pkill -x` nie ma tu
opisywanego wcześniej problemu. Launcher/desktop poza repozytorium może nadal
mieć etykietę „Idle WalkingPad 2D"; nie zmieniaj plików systemowych w ramach
tego projektu. Istniejący launcher preferuje binarkę `target/release` po jej
przebudowaniu.

Mostek BLE (`walkingpad_to_joy.py`) zapisuje km/h do
`/tmp/walkingpad-speed`; gra nie łączy się z BLE bezpośrednio. Plik starszy niż
5 s oznacza postój.
