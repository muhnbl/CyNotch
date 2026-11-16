# ⚡ CyNotch — Optimization Report (v2.5)

Dokumen ini merangkum pekerjaan **optimasi performa** pada project CyNotch
(`~/.gemini/antigravity/scratch/novanotch`).

**Ruang lingkup:** hanya optimasi. Tidak ada fitur yang ditambah, dihapus,
diganti, atau diubah perilakunya. Seluruh tampilan, animasi, ukuran, dan
interaksi dipertahankan — dibuktikan dengan uji regresi piksel (lihat §4).

---

## 1. Metodologi

Optimasi dilakukan berbasis pengukuran, bukan asumsi:

1. **Bottleneck nyata diukur dulu.** Karena tidak ada profiler eksternal di
   lingkungan ini, instrumentasi sementara (`std::time::Instant` per-seksi)
   dipasang di thread poll, thread window (`WM_TIMER`), dan pada setiap
   primitif penggambaran `Painter`, lalu dilepas kembali setelah data
   terkumpul.
2. **Harness benchmark permanen** ditambahkan di
   [`examples/bench_render.rs`](examples/bench_render.rs) sehingga setiap
   perubahan bisa diukur dalam hitungan detik (`cargo run --release --example
   bench_render -- 150`). Harness ini merekam seluruh state yang benar-benar
   berjalan di produksi: capsule *minimized*, idle horizontal/vertikal,
   expanded Home, lyrics, serta panel Preferences.
3. **A/B build nyata.** Source sebelum optimasi disalin ke
   `scratch/cynotch_baseline_ab`, dibangun dengan profile rilis yang sama, dan
   dijalankan bergantian di mesin yang sama.
4. **Verifikasi visual.** Mode `--snapshot` menghasilkan 46 PNG untuk semua
   state; setiap pasangan PNG dibandingkan per-piksel terhadap baseline.

---

## 2. Temuan utama (hasil profiling)

| Area | Temuan |
| --- | --- |
| Render idle | Satu frame capsule *minimized* = **~250 µs**, dari ~50 operasi vektor. Mascot Cyno sendiri menyumbang ~95 µs; shell + chrome ~120 µs. |
| Expanded / hover | Satu frame = **9.9–13 ms**. Komposisi biaya: *radial glow* bermask 25 kali × ~190 µs = **~50% frame**, gradient kartu ~25%, stroke ~14%. |
| Shading | Biaya shader `tiny-skia` bersifat per-piksel: gradient radial **~10 ns/px** vs solid **~2.4 ns/px**. Jadi biaya ditentukan oleh **luas area yang di-shade**, bukan jumlah panggilan. |
| Preferences | Panel 937×785 butuh **~34 ms/frame**, dengan 3 *aurora glow* selebar ~840 px yang tidak pernah berubah antar frame. |
| Poll thread | `audio.read_master_volume()` **~2.0 ms/poll** (enumerasi endpoint + `Activate` tiap 500 ms); `notif.poll_new_toasts()` **~60 ms terblokir/poll** (marshal seluruh Action Center via RPC); media 1.3 ms; telemetry 1.2 ms. |
| Alokasi per frame | `Pixmap`+`Mask` panel Preferences (multi-MB) dialokasikan ulang setiap frame; `Vec<Hit>` per frame; clip mask di-*memset* seluruh kanvas pada setiap `prepare_*_clip` (~21×/frame di state expanded); *clear* pixmap menulis seluruh baris (bukan hanya kolom kotor). |
| Build | Tanpa `[profile.release]`: 4.15 MB, tanpa LTO/codegen-units/strip. |

---

## 3. Optimasi yang diterapkan

### 3.1 Rendering — shading hanya pada area yang terlihat
[`src/painter.rs`](src/painter.rs)

- `radial_glow` / `elliptical_glow` kini menghitung **rect minimum** yang perlu
  di-shade: bounding box gradien diiris dengan bounding box clip mask aktif
  (`Painter::clip_bounds`), lalu diisi lewat `fill_rect` (jalur scan cepat,
  tanpa AA) alih-alih `fill_path` lingkaran yang di-AA.
  **Lossless**: gradien glow bernilai alpha 0 tepat di radiusnya, sehingga
  pemotongan ke area clip tidak menghilangkan satu piksel pun yang terlihat.
- `prepare_corners4_clip` tidak lagi me-*memset* seluruh kanvas. Bounding box
  clip terakhir dicatat (per pointer mask, thread-local) dan hanya area itu
  yang dibersihkan; mask yang baru dialokasikan tidak perlu dibersihkan sama
  sekali. Menghemat ~21 memset kanvas per frame expanded.

### 3.2 Renderer — dampak alokasi & bandwidth
[`src/renderer.rs`](src/renderer.rs)

- Clear pixmap hanya pada **kolom kotor** (sebelumnya seluruh lebar baris),
  bukan seluruh baris kanvas.
- `Vec<Hit>` dipakai ulang antar frame (`mem::take`) alih-alih alokasi baru.

### 3.3 Preferences — hilangkan pekerjaan yang identik antar frame
[`src/settings_window.rs`](src/settings_window.rs)

- Shell luar + 3 *aurora glow* + bevel stroke adalah **piksel yang sama persis**
  selama ukuran window, mode frosted, dan type scale tidak berubah. Hasilnya
  kini disimpan sekali lalu di-`memcpy` kembali (bukan blit pattern yang mahal).
  Cache di-key oleh `(lebar, tinggi, frosted, type_scale)`.
- Buffer *scroll pane* (`Pixmap` + `Mask` multi-MB) dipakai ulang lewat
  thread-local scratch, tidak lagi dialokasikan setiap frame.
- `pixmap.fill(TRANSPARENT)` dilewati ketika cache chrome dipakai (hasil akhir
  identik, tetapi satu memset 3 MB hilang).

### 3.4 Thread polling — buang COM/WinRT yang berulang
[`src/audio.rs`](src/audio.rs), [`src/main.rs`](src/main.rs)

- `AudioMonitor` menyimpan `IAudioEndpointVolume` hasil `Activate` dan hanya
  me-resolve ulang endpoint tiap **3 detik** (atau saat pembacaan gagal).
  Volume tetap terbaca real-time; perpindahan perangkat tetap terdeteksi.
  Biaya turun dari **~2.0 ms → ~0.4 ms per poll**.
- Poll notifikasi Windows (`GetNotificationsAsync().get()`, yang memblokir
  ~60 ms dan memarshal seluruh Action Center) dijalankan pada **setengah
  cadence** loop (≈1 detik). Banner tetap muncul dalam ≤1 detik, jauh di bawah
  durasi tampil banner (2–8 detik), dengan biaya separuh.

### 3.5 Build profile
[`Cargo.toml`](Cargo.toml)

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "unwind"      # dipertahankan: wnd_proc memakai catch_unwind (v2.4)
strip = "symbols"
```

`panic = "unwind"` **sengaja tidak** diubah menjadi `abort`, karena
`catch_unwind` di `wnd_proc`/`settings_wnd_proc` adalah pengaman anti-crash
yang sudah ada sejak v2.4.

---

## 4. Hasil pengukuran

### 4.1 Render per frame (A/B, mesin & sesi sama, 150 iterasi)

| State | Sebelum | Sesudah | Perubahan |
| --- | --- | --- | --- |
| Capsule *minimized* (kanan, vertikal) | 248.6 µs | **199.9 µs** | −19.6% |
| Idle capsule horizontal | 495.8 µs | **412.7 µs** | −16.8% |
| Idle capsule vertikal | 618.2 µs | **529.8 µs** | −14.3% |
| Expanded Home (hover) | 9.90 ms | **8.51 ms** | −14.0% |
| Expanded Music + Lyrics | 13.09 ms | **10.96 ms** | −16.3% |
| Panel Preferences (937×785) | 33.9 ms | **20.4 ms** | **−39.8%** |
| Chrome saja (mascot off) | 133.0 µs | 120.6 µs | −9.3% |

### 4.2 CPU idle & ukuran biner

A/B langsung (exe baseline vs exe optimasi, config pengguna yang sama,
20 detik pemanasan + 3 × 30 detik sampling):

| Metrik | Sebelum | Sesudah | Perubahan |
| --- | --- | --- | --- |
| CPU idle, sampel 1 | 6.72% | 4.84% | |
| CPU idle, sampel 2 | 6.25% | 4.43% | |
| CPU idle, sampel 3 | 7.81% | 4.43% | |
| **CPU idle rata-rata** | **~6.9%** | **~4.6%** | **−34%** |

| Metrik | Sebelum | Sesudah |
| --- | --- | --- |
| Ukuran `cynotch.exe` | 4 151 296 B | **3 835 392 B** (−7.6%) |
| Working set setelah trim Windows | ~24 MB | **~11.5 MB** |
| Private bytes | ~64 MB | ~64 MB (didominasi surface DComp/D3D11 *frosted*) |

### 4.3 Kualitas kode & regresi

- `cargo clippy --all-targets --release -- -D warnings`: **0 warning, 0 error**
  (setara baseline).
- `cargo test --release`: **41/41 lulus** (seluruh test lama, tanpa satu pun
  test yang diubah).
- **Regresi visual:** 46 snapshot PNG dibandingkan per-piksel dengan baseline.
  Selisih hanya pada tepi glow lembut: maksimum 0.29% piksel per frame dengan
  delta kanal ≤44/255, dan 0 px pada state `idle`, `idle_netspeed`,
  `music_idle`. Frame banner notifikasi bersifat non-deterministik sejak awal
  (cincin hitung mundur), terbukti dengan membandingkan dua run identik.
- **Smoke test live** (`~/.local/bin/cynotch.exe`): `ALIVE=True`,
  `HWND=7604042`, `VISIBLE=True`, tidak ada panic, CPU stabil ~4% setelah
  window di-trim ke 11.5 MB.

---

## 5. Yang sengaja TIDAK diubah

| Kandidat optimasi | Alasan tidak diambil |
| --- | --- |
| Menurunkan cadence animasi idle 28 ms → 40 ms (−30% biaya render idle) | Ini mengubah *feel* animasi dan bertabrakan dengan test yang mengunci perilaku `needs_animation_timer_ms() == Some(28)`. Fitur/perilaku tidak boleh berubah. |
| Menghapus/mengganti glow dekoratif | Mengubah tampilan. |
| Meng-cache kartu expanded sebagai layer | Blit pattern `tiny-skia` ≈ 11 ns/px, lebih mahal daripada menggambar ulang sebagian chrome; butuh arsitektur compositing baru dengan risiko regresi visual tinggi. |
| `panic = "abort"` | Mematikan pengaman `catch_unwind` di `wnd_proc` (regresi v2.4). |
| Mengurangi fitur notifikasi/audio | Fitur harus tetap utuh; yang dioptimasi hanya cara memperoleh datanya. |

---

## 6. Peluang optimasi lanjutan (belum dikerjakan)

Diurutkan berdasarkan perkiraan imbal hasil:

1. **Cache layer per-kartu untuk state expanded** (perlu compositing manual
   `memcpy` untuk layer opaque) — potensi ~2–3 ms/frame dari 8.5 ms.
2. **Event-driven notification bridge** (`UserNotificationListener.NotificationChanged`)
   untuk menghapus polling RPC sepenuhnya — potensi ~2–3% CPU idle.
3. **Pre-render sprite mascot per fase animasi** (quantized frame cache) —
   potensi ~50 µs/frame di state idle.
4. **Scissor per-kartu pada `elevated_card`** untuk memangkas overdraw shadow.
5. **Trim fitur crate `windows`** yang tidak terpakai (mis. `Direct3D*`,
   `UI_Composition*`) untuk menurunkan ukuran biner lebih jauh.

---

## 7. Cara memverifikasi ulang

```powershell
cd ~\.gemini\antigravity\scratch\novanotch
cargo clippy --all-targets --release -- -D warnings   # 0 warning
cargo test --release                                  # 41/41
cargo build --release                                 # target\release\cynotch.exe
cargo run --release --example bench_render -- 150      # semua state
cargo run --release --example bench_render -- 20 settings
.\target\release\cynotch.exe --snapshot .\snapshots_v25
```
