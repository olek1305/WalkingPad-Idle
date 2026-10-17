# PLAN — Biomy v2: regiony co 300 m, świat bez wrapu

## Diagnoza (czemu znika i miga)

1. Wszystko wisi na `scroll` (f32, piksele) z wrapem co 117000 px. Przy 100 km/h
   wrap co 25 s → cała sekwencja 78 slotów ZACZYNA SIĘ OD NOWA. Widać powtórki
   i „znikające” miasto/las.
2. Slot 1500 px ≈ 9 m przy 5 km/h — region zmienia się co ~6 s. Za szybko.
3. Hash `ki = k % 78` daje tylko 78 wariantów — świat się zapętla, nie jest nieskończony.

## Pomysł: regiony z DYSTANSU + współrzędne świata w f64

- `cam_m: f64` — kamera = przebyte metry (rośnie z `total_km`, nigdy nie wrapuje).
- Region: `idx = floor(cam_m / 300)`, biom = `hash(idx)`:
  LAS 35% / ŁĄKA 25% / WIEŚ 20% / MIASTO 20%.
- Obiekty mają ABSOLUTNE pozycje w metrach; ekran `x = (obj_m - cam_m) * PX_M`.
  Różnica liczona w f64 → precyzja idealna ZAWSZE, wrap niepotrzebny wcale.
- `PX_M ≈ 165 px/m` (to samo tempo co dziś: 5 km/h = 230 px/s).
- Region 300 m ≈ 3,6 min marszu @5 km/h. Obiekt raz postawiony STOI, aż sam
  nie wyjedzie za ekran — nic nie znika w połowie.

## Pasowanie (transitions)

- Granica biomu to linia w świecie; widać maks. 2 regiony naraz — grunt rysowany
  spanami z mapy regionów (zero szwów).
- Wejście do lasu: „ściana lasu” — zwarty rząd ciemnych świerków na granicy.
- Drogowskaz z PRAWDZIWYM dystansem: „WIEŚ 200 m” (licznik maleje z marszem).
- Świetliki w lesie o zmierzchu (żółte kropki na sinusach) — tanie i ładne.

## Helpery (porządek w kodzie)

- `struct Rng(u64)` — `next() / range(a,b) / chance(p)`; koniec z `wrapping_*` rozsypanymi po kodzie.
- `fn hash(i64) -> u64` — jeden splitmix64 dla całego świata.
- `enum Biom { Las, Laka, Wies, Miasto }`, `fn biom(idx: i64) -> Biom`.
- `fn button(...)` — jeden helper na klikane prostokąty drzewka (mniej powtórek).
- Nowe grafiki w tym planie: ściana lasu, drogowskazy z metrami, świetliki,
  grunt/leśna ściółka (szyszki? — 3 pixele, czemu nie).

## Logika kroków

1. `Rng`, `hash`, `Biom`, `cam_m` + `PX_M`; `wt` bez zmian.
2. Ziemia spanami z mapy (las/grunt vs asfalt), słupki co 300 m w tle? nie — znaki.
3. Drzewa/trawa/pasy globalnie co X metrów + zagęszczenie w lesie; kreski tylko poza lasem.
4. Osady: 1 na region WIEŚ/MIASTO (start + 50–150 m); las: gęstwina + grzyby + zwierzyna jak dziś.
5. Znaki, ściana lasu, świetliki.
6. Test: `--sim 100` 30 s (przelot ~833 m ≈ 3 regiony) + screenshot; zapis kompatybilny.

## Co NIE ruszam

Ekonomia 1:1, drzewko, zapis, mostek, skala 0–20. Stary `scroll`-wrap i `rem_euclid`
znikają (niepotrzebne), `phase %= TAU` zostaje.
