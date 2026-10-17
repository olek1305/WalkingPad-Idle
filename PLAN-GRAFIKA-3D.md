# Plan: grafika 3D, światło, cienie i fizyka

Stan bieżący (2026-10-09): etapy 1–10 są ukończone. Ostatnia pełna weryfikacja sprzed poprawek latarni obejmowała 87 testów, formatowanie, Clippy z `-D warnings`, kompilację shaderów GLSL i build release. Późniejsze poprawki latarni były tylko formatowane i kompilowane, bez nowych testów i benchmarków na polecenie użytkownika. Nadzór wcześniej uruchomił sceny kontrolne na Intel Arc B580 oraz zmierzył wersję release. Uzupełniono animowany zegar diagnostyki, rzeczywisty czas benchmarku, przenośny skrypt shaderów, interpolację kolorów w siatkach terenu i dokładniejsze mapy cieni; usunięto twarde pierścienie nieba. Szczegółowe dowody i ograniczenia znajdują się poniżej oraz w CLAUDE.md.

Korekty z podglądu użytkownika: ramiona przeniesiono z wnętrza tułowia na jego boki, a miejską deskę/parapet pod dolną krawędź okien. Dwa dodatkowe testy obejmują prześwit ramion w pełnym cyklu przy postoju, marszu i biegu oraz położenie parapetu na obu fasadach od drogi. Kolory, wspólna geometria cieni, fizyka i zapis pozostają bez zmian.

Nadzór po etapie 6 dostarczył wcześniejsze dowody: capture day/night 2048×1152/high bez acne na ziemi oraz debug benchmark rain/medium 59,9 FPS. Końcowa weryfikacja release zastępuje ten wstępny pomiar: nocny deszcz w oknie 1883×2123 osiąga 60,0 FPS na medium i 59,7 FPS na high. Kolory pod lampami, sylwetki cieni, świt, mgłę, fizykę i płynne granice biomów sprawdzono w rzeczywistych capture. Grafika pozostaje proceduralna/low-poly, a fizyka korzysta z przybliżonych colliderów i płaskiego podłoża — nie jest to deklaracja fotorealizmu ani pełnej symulacji każdej kropli.

Sekcja „Stan projektu przed zmianami — audyt” poniżej jest historycznym punktem wyjścia. Opisuje stan sprzed etapów, a nie bieżącą architekturę.

## Ustalenia użytkownika

- Usunąć świat gry 2D; docelowo jedna scena 3D, z kamerą dającą czytelny efekt 2,5D.
- Poprawić wygląd obiektów, dopasowanie skali, światło latarni i cienie zależne od kształtu.
- Zachować rozpoznawalne barwy materiałów. Cień powinien zachowywać kolor podłoża i światło otoczenia.
- Dodać silnik fizyki, grawitację i kolizje.
- Implementacja: dokładnie GPT-5.6 Luna, reasoning effort `max`; agent prowadzący planuje, przegląda kod i weryfikuje wynik.
- Potwierdzony zakres: automatyczny marsz z bieżni oraz fizyka postaci i otoczenia.

## Stan projektu przed zmianami — audyt

Projekt używa Rust edition 2024 i macroquad 0.4.16. Renderer 3D istnieje w `src/render3d/mod.rs`, ale jest dostępny przez `--3d`; domyślny start wciąż wybiera 2D.

`cargo test --offline --locked`: 27 testów przechodzi. Testy obejmują ekonomię, zapis, questy, świat proceduralny, dobę, pogodę i polskie formatowanie. Nie potwierdzają poprawności obrazu ani fizyki.

### Rozbieżności w CLAUDE.md i komentarzach

- Opis architektury w CLAUDE.md:21 jest nieaktualny: renderowanie 2D jest w `src/render/`, UI w `src/ui/`, a renderer 3D nie został opisany.
- Opis grafiki i przeliczenie ekranu przez `PX_M` dotyczą 2D. W 3D kamera i geometria używają metrów.
- `daynight::init_offset()` liczy godzinę z sekund epoki modulo 86400, czyli UTC, mimo opisu startu z godziny systemowej.
- Funkcje `animals()` i `settlement()` w rendererze 3D są nieużywane. Scena nie rysuje także grzybów, chociaż album i questy nadal je rozliczają.
- W `main.rs:296` pozycja biegacza dla pogody i questów nadal zależy od szerokości okna i przelicznika 2D. Model 3D jest umieszczony przy lokalnym x = 0.
- `light_pool()` opisuje dodawanie światła, ale używa tego samego mieszania alfa co cienie. Plama przemalowuje podłoże; nie jest oświetleniem jego materiału.
- `Save::quests` ma stary komentarz o nieużywanej rezerwie, chociaż questy działają.
- Wzmianka o streak ×1,5 w nagłówku `main.rs` nie odpowiada ekonomii w kodzie.
- Launcher rzeczywiście startuje grę bez `--3d`, a plik desktop ma nazwę „Idle WalkingPad 2D”. Nie modyfikowano tych plików poza repozytorium.

Potwierdzono w kodzie stawki i koszty ulepszeń, bazę 1:1, regiony 300 m, dobę 2000 m, odczyt prędkości, okres zapisu i skróty F/T/Esc. Opisy sprzętowych limitów modeli bieżni nie są podstawą tego planu.

## Etap 1 — jedna scena i spójny świat

1. Ustawić 3D jako jedyny renderer świata i zmienić tytuł okna.
2. Usunąć moduły rysowania świata 2D oraz obsługę `--2d`. Opcjonalne `--3d` pozostawić jako zgodną wstecznie flagę bez efektu.
3. Zachować nakładkę HUD, sklep, album i efekty UI.
4. Wyprowadzić wspólny opis obiektów regionu: pozycja, geometria, materiał, collider, źródło światła, uczestnictwo w rzucaniu cieni.
5. Ujednolicić pozycję postaci dla kamery, pogody i questów. Szerokość okna nie może zmieniać regionu postaci.
6. Podłączyć osady, zwierzęta i grzyby do sceny 3D. Zachować dotychczasowe typy i deterministyczne identyfikatory albumu; pozycje obiektów wyrażać w metrach i umieszczać w odpowiednich regionach.

Odbiór: start bez flag uruchamia 3D; zapis i wszystkie istniejące testy działają; widoczna zawartość świata zgadza się z albumem.

## Etap 2 — geometria, materiały i kolory

1. Rozdzielić obecny monolityczny renderer na geometrię, świat, postać, materiały, oświetlenie, cienie i pogodę.
2. Przechowywać w siatkach bazowy kolor materiału, normalne oraz parametry chropowatości i emisji. Oświetlenia i mgły nie wypalać w kolorach wierzchołków na CPU.
3. Ulepszyć proporcje postaci, zaokrąglenia kończyn, sylwetki drzew i detale zabudowy; dopasować skale obiektów w metrach.
4. Zdefiniować paletę materiałów przez odcień, nasycenie i jasność; do korekt artystycznych używać HSL lub OKLCH. Obliczenia światła prowadzić w liniowym RGB, z pojedynczą konwersją do sRGB przy wyświetlaniu świata.
5. Rozdzielić materiał asfaltu, ziemi, liści, drewna, metalu, skóry i tkaniny. Mokrość zmienia chropowatość oraz odbicia w sposób ciągły.
6. Oddzielić transformacje kolorów świata od kolorów UI. Ograniczać prześwietlenia bez obcinania każdego kanału osobno.

Odbiór: materiały zachowują rozpoznawalne barwy w dzień, w cieniu i pod latarnią; jasne miejsca nie tracą koloru; sylwetki nie składają się wyłącznie z ostrych prostopadłościanów.

Uwaga: ciepła lampa naturalnie wpływa na widziany odcień. Celem jest spójna, kontrolowana zmiana koloru, a nie absolutnie identyczny hue pod każdym źródłem światła.

## Etap 3 — rzeczywiste oświetlenie powierzchni

1. Dodać shader powierzchni: światło otoczenia z nieba, słońce i księżyc, lokalne latarnie, składnik rozproszony i odbicia zależne od chropowatości.
2. Każda latarnia ma pozycję klosza, kierunek, kąt stożka, zasięg, barwę i moc. Obliczać spadek energii z odległości oraz płynne przejście na brzegu stożka.
3. Do shaderów wybierać ograniczoną liczbę istotnych lamp. Punkt wyjścia: do 8 lokalnych świateł i do 2 pobliskich lamp rzucających cienie, z płynnym przełączaniem.
4. Oddzielić emisję klosza i okien od rozjaśniania innych obiektów. Usunąć nakładki zastępujące oświetlenie gruntu.
5. Mgłę zastosować po obliczeniu oświetlenia. Snop lampy pokazywać subtelnie w deszczu i mgle, zgodnie z widocznością źródła.

Odbiór: przejście pod lampą rozjaśnia odpowiednią stronę postaci, drogę i pobliskie obiekty; obiekty poza stożkiem nie otrzymują tego samego światła. Zasłanianie źródła przez przeszkody zostaje potwierdzone w etapie 4.

## Etap 4 — cienie zgodne z geometrią

1. Zastąpić rzut wypukłego obrysu całej bryły mapami cieni renderowanymi z faktycznych trójkątów. Ten sam model i poza postaci służą do rysowania i rzucania cienia.
2. Użyć mapy kierunkowej dla słońca/księżyca i map perspektywicznych dla wybranych latarni. Najpierw potwierdzić działanie na aktualnym backendzie GPU.
3. W macroquad 0.4.16 render target może mieć bufor głębi, ale jego `render_pass.depth_texture` jest ustawiane na `None`. Nie zakładać, że można odczytać tę teksturę przez publiczne pole. Bezpieczny punkt wyjścia: zapisać głębię do kolorowej tekstury w passie z testem głębi.
4. Użyć mapy 2048 px dla światła kierunkowego i 512 px dla lamp w jakości medium oraz 4096/1024 w high; stosować PCF do wygładzenia krawędzi, korektę bias i stabilizację mapy przy ruchu kamery.
5. Widoczność źródła pomniejsza tylko jego wkład w oświetlenie. Światło nieba i pozostałych lamp nadal trafia do materiału; nie nakładać prawie czarnego wielokąta na podłoże.
6. Zapewnić cienie na ścianach i własnych powierzchniach obiektów oraz subtelne przyciemnienie kontaktowe przy stopach i podstawach.

Odbiór: widać oddzielne ręce i nogi oraz przerwy w sylwetce; kierunek i długość cienia wynikają z położenia źródła; kilka źródeł może wypełniać wzajemne cienie; brak migotania i pasów na powierzchniach.

## Etap 5 — fizyka i animacja

1. Dodać `rapier3d` jako silnik fizyki niezależny od renderowania.
2. Użyć metrów, grawitacji około -9,81 m/s² i stałego kroku 1/60 s z akumulatorem czasu oraz interpolacją renderowania. Ograniczyć liczbę kroków po długiej przerwie.
3. Dodać collider podłoża, kolidery stałych obiektów oraz kapsułę i kontroler postaci. Ruch poziomy pochodzi z prędkości bieżni. Dla kontrolera kinematycznego jawnie integrować prędkość pionową i grawitację.
4. Dopasowywać chód i kontakt stóp do podłoża; ograniczyć ślizganie stóp, dopasować kolana i biodra oraz wygładzić start i zatrzymanie animacji.
5. Dodać widoczne zastosowanie fizyki otoczenia: ograniczoną pulę małych dynamicznych obiektów z grawitacją i kolizjami. Deszcz, liście, pył i rozbryzgi integrować jako lekkie cząstki z wiatrem, oporem i reakcją na podłoże.
6. Zachować absolutną pozycję trasy w f64. Symulować fizykę lokalnie i okresowo przesuwać jej początek razem z colliderami oraz stanem interpolacji, aby długie sesje nie traciły precyzji.
7. Efekty fizyczne kończą ruch także po zatrzymaniu taśmy. Postęp trasy i ekonomia nadal wynikają z prędkości bieżni.

Odbiór: obiekt upuszczony nad drogą spada i zatrzymuje się na colliderze; postać nie zapada się w podłoże; wynik fizyki jest zgodny przy różnych FPS; ruch wizualny nie generuje dodatkowego dystansu ani golda.

## Etap 6 — spójny wygląd i wydajność

1. Dopasować kamerę do obszaru gry obok panelu i aktualnej proporcji okna.
2. Dopracować niebo, daleki teren, przejścia biomów, deszcz, mgłę, mokry asfalt i światło zmierzchu.
3. Buforować geometrię statycznych regionów. Aktualizować światło na GPU; nie budować od nowa całego świata i obrysów cieni w każdej klatce.
4. Odrzucać obiekty poza widokiem i zasięgiem światła; stosować uproszczenia geometrii dla oddalonych obiektów i limit dynamicznych ciał.
5. Zmierzyć czas klatki, liczbę siatek i koszt cieni w lesie oraz mieście. Cel roboczy: stabilne 60 FPS przy 1280×720 na maszynie użytkownika, do potwierdzenia pomiarem.

Odbiór: złożona nocna scena z deszczem zachowuje czytelność i stabilne tempo; zmiana jakości cieni nie usuwa ich podstawowej zgodności z kształtem.

## Etap 7 — implementacja i dowody

Ukończono:

- walidację CLI `Result` dla scen, jakości, capture, benchmarku i `--sim`;
  błędne argumenty kończą się komunikatem przed `load_save()`, a `--2d`/`--3d`
  pozostają no-opami;
- save-free dla każdej diagnostyki, także Escape, oraz stały krok 1/60 s w
  capture; normalna gra i mostek zachowują czas rzeczywisty oraz dotychczasową
  ekonomię i schemat Save JSON;
- sceny `day`, `dawn`, `night`, `rain`, `fog`, `physics`, `two-lights` i
  `boundary`, zakotwiczone w rzeczywistych ID/pozycjach lamp i granicach biomów;
- symetryczne przejście koloru i materiału biomów, ciągłą okresową trajektorię
  źródła słońca oraz chmury o spłaszczonej, przyciemnianej nocą geometrii;
- zgodny wybór LOD dla passów koloru/cienia i użycie low/medium/high map w
  stabilizacji projekcji oraz rozmiarach texeli PCF;
- testy fizyki postoju/start-stop/4/20 km/h, szybkiego przejścia regionów,
  originu 1e9 m, cache, biomów, projekcji lamp/kierunku, kolorów i parsera.

Uruchomione dowody etapu 7:

```text
cargo fmt --all -- --check                              PASS
cargo test --offline --locked                            PASS (83 tests)
cargo clippy --offline --locked --all-targets -- -D warnings  PASS
cargo build --offline --locked --release                 PASS
bash scripts/check-shaders.sh                            PASS
```

Statusy powyżej zostały ponownie sprawdzone przez nadzór 2026-10-09.

Odbiór GPU na Intel Arc B580/Mesa 26.2.2:

- sceny `day`, `dawn`, `night`, `rain`, `fog`, `physics`, `two-lights`,
  `boundary` uruchomiły się w release bez błędów shaderów; sprawdzono obrazy;
- widoczne są cienie z faktycznej geometrii i barwy materiałów pod lampami,
  brak wcześniejszych prążków podłoża, płynne przejścia biomów i ciemne chmury nocą;
- upuszczony obiekt przy 0 km/h był nad drogą w pierwszej klatce i spoczywał
  na niej po 120 klatkach; postać i ekonomia nie wymagają ruchu taśmy do
  dokończenia symulacji grawitacji;
- stara flaga `--2d` nie wybiera świata 2D; przelot diagnostyczny pokazał
  las, grzyby i odkrycia albumu w scenie 3D;
- błędna scena kończy się kodem 2 jeszcze przed inicjalizacją okna;
- zapis użytkownika pozostał niezmieniony podczas końcowych testów.

Pomiary release: 1 s rozgrzewki + 6 s pomiaru, jedna testowana instancja.
Rozmiary poniżej dotyczą całego okna, nie samego obszaru świata. Czas CPU
renderera jest oddzielnym pomiarem, a nie czasem GPU.

| Test | Jakość | Okno | Średnie FPS | p95 klatki | CPU renderera |
| --- | --- | --- | --- | --- | --- |
| Nocny deszcz, 4,2 km/h | medium | 1883×2123 | 60,0 | 17,07 ms | 2,94 ms |
| Nocny deszcz, 4,2 km/h | high | 1883×2123 | 59,7 | 17,34 ms | 3,03 ms |
| Przelot przez lasy/osady, 1440 km/h | medium | 3840×2123 | 60,0 | 17,22 ms | 1,80 ms |

Cache pozostawał ograniczony do 5 regionów i 27–28 kafli w tych scenach.
Jakość low/medium/high używa map kierunkowych 1024/2048/4096 i map lamp
256/512/1024. Są to krótkie lokalne pomiary, nie gwarancja dla innych GPU.

## Etap 8 — dopracowanie animacji po odbiorze sylwetki

Użytkownik zaakceptował boczne ramiona i parapet pod oknami. Kolejna korekta
dotyczy ruchu, bez zmiany zaakceptowanych pozycji bocznych, materiałów,
fizyki nóg i zabudowy:

1. Usunąć rozciąganie odcinków rąk w trakcie chodu; obracać kończyny o stałych długościach.
2. Płynnie dobierać wymach i ugięcie łokci do wygładzonej prędkości z pozy fizyki; usunąć skok animacji przy 0,05 km/h.
3. Powiązać pochylenie tułowia z tą samą wygładzoną prędkością.
4. Sprawdzić stałe długości, prześwit, symetrię wymachu i ciągłość fazy/prędkości testami; odebrać obraz postoju, marszu i biegu.

Status: implementacja ukończona przez Lunę 5.6 Max. Odcinki rąk mają stałe
długości 0,35/0,29 m; płynny wymach i pochylenie korzystają z wygładzonej
prędkości pozy fizyki. Zachowano zaakceptowane pozycje boczne, kolory,
parapety oraz fizykę nóg. Przed poleceniem pominięcia testów przeszło 87
testów i obejrzano postój, marsz, bieg oraz lampy. Na dalsze etapy użytkownik
polecił nie uruchamiać testów ani benchmarków; pozostaje kompilacja potrzebna
do uruchomienia gry. Nowego benchmarku nie wykonano.

## Etap 9 — krótkie wysięgniki latarni nad jezdnią

Ukończono przez Lunę 5.6 Max: usunięto podwójne doliczanie pozycji bocznej
do modelu klosza i zastąpiono je wspólnym przesunięciem względem słupa.
Wysięgnik ma 0,9 m, słup stoi 0,5 m poza krawędzią drogi, a klosz nad
asfaltem na ±2,2 m. Obie strony kierują ramię do środka drogi. Model,
rzeczywiste źródło światła i jego cienie korzystają ze spójnych pozycji;
zachowano kolory, moc, identyfikatory i kierowanie stożka na jezdnię.

Na polecenie użytkownika wykonano tylko formatowanie i kompilację release,
bez nowych testów, benchmarków i capture. Wcześniejsze wyniki 83/87 testów
oraz pomiary FPS dotyczą wcześniejszych etapów, nie tej korekty.

## Etap 10 — nowoczesna oprawa LED

Ukończono przez Lunę 5.6 Max: pomarańczową kulę zastąpiła płaska grafitowa
obudowa ze ściętymi narożnikami i wpuszczonym białym panelem od spodu.
Zachowano krótkie ramię nad jezdnią, pozycje słupów i środek emisji;
orientacja panelu odpowiada kierunkowi rzeczywistego światła. Obudowa i
mocowanie nie przecinają osi emisji. Oba LOD-y zachowują nową sylwetkę.

Panel oraz źródło używają wspólnego białego koloru; osobna flaga NIGHT_LED
zapewnia emisję nocną bez pomarańczowego przefarbowania i bez zmiany okien.
Na polecenie użytkownika wykonano tylko formatowanie i kompilację release,
bez testów, benchmarków, capture i walidacji shaderów.

## Nadzór nad Luną

Agent prowadzący przydziela po jednym etapie z określonymi plikami i warunkami odbioru. Po wykonaniu analizuje zmiany, uruchamia weryfikację i przekazuje poprawki przed kolejnym etapem.

GPT-5.6 Luna nie występuje na liście modeli wbudowanego narzędzia delegowania w tej sesji. Implementacja korzysta z lokalnego Codex CLI z `-m gpt-5.6-luna` oraz `-c 'model_reasoning_effort="max"'`. Uruchomienie kontrolne potwierdziło obie wartości w nagłówku sesji i otrzymało odpowiedź modelu. Innego modelu nie podstawiać automatycznie.

## Źródła techniczne

- [GPT-5.6 Luna — OpenAI Docs](https://developers.openai.com/api/docs/models/gpt-5.6-luna): identyfikator modelu i poziom rozumowania max.
- [Vertex — macroquad 0.4.16](https://docs.rs/macroquad/0.4.16/macroquad/models/struct.Vertex.html): normalne przekazywane do własnych shaderów.
- Render target i materiały sprawdzono w lokalnych źródłach macroquad 0.4.16; szczególnie `src/texture.rs:421` i `src/material.rs`.
- [Kontroler postaci — Rapier](https://rapier.rs/docs/user_guides/rust/character_controller/): ruch kinematyczny i korekta względem przeszkód.
- [Krok symulacji — Rapier](https://rapier.rs/docs/user_guides/rust/integration_parameters/): skala w metrach i krok fizyki.
