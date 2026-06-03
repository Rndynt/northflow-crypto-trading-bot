# Readiness Review — 2026-05-13

## Kesimpulan singkat
Belum layak langsung live-trading dengan leverage tinggi tanpa hardening tambahan. Arsitektur risk/exit/duplikasi sudah cukup baik untuk paper/sandbox, tetapi masih ada gap sinkronisasi posisi exchange vs local state dan tidak terlihat proteksi tegas untuk *single-position-per-symbol* di level exchange sebelum entry dieksekusi.

## Temuan utama

1. **Anti-duplicate posisi sudah ada di RiskAgent (by symbol)**
   - Pada event `PreSignalEmitted`, RiskAgent menolak sinyal jika symbol sudah ada di `open_symbols`.
   - Reason: `position already open for <symbol>`.

2. **Idempotency order ada, tapi hanya mengurangi retry duplicate dalam bucket waktu**
   - `idempotent_client_id(...)` deterministik dalam bucket 1 menit.
   - Ini bagus untuk mencegah double-submit identik, namun tidak menjamin tidak ada posisi tambahan bila sinyal beda ukuran/harga atau beda bucket.

3. **Execution membuka posisi baru tanpa cek eksplisit posisi symbol yang sudah terbuka di PositionBook tepat sebelum place_order**
   - ExecutionAgent menerima `ManagerVerdictEmitted`, validasi bracket & survival/risk block, lalu langsung `place_order`.
   - Tidak ada guard final `if symbol already open => skip/close/merge` di titik eksekusi.

4. **SL/TP protektif cukup kuat**
   - Setelah entry fill, agent memasang `STOP_LOSS` + `TAKE_PROFIT` reduce-only.
   - Jika setup protective order gagal dan mode required aktif, sistem freeze trading.

5. **Manajemen posisi intratrade sudah baik (partial TP, BE, trailing, time exit)**
   - Partial TP 50% di 1R, SL pindah BE, trailing aktif lalu update SL dinamis.

6. **Learning loop aktif dan berdampak ke gating/size**
   - Lesson extractor bisa block/derate/boost strategy-regime-symbol.
   - LearningPolicy diaplikasikan oleh RiskAgent sebelum Brain/Manager lanjut.

## Jawaban atas kekhawatiran Anda

### A) Apakah bisa terjadi duplikasi / menimpa posisi?
- **Duplikasi by symbol secara desain: dicegah di RiskAgent** (lapisan awal).
- **Namun** masih ada kemungkinan edge case jika state `open_symbols` terlambat sinkron (race/restart/reconcile delay), karena final gate di Execution belum melakukan re-check kuat terhadap posisi simbol terbuka pada exchange saat itu.
- Ini berarti risiko penambahan margin (*position stacking*) **masih mungkin** di kondisi abnormal.

### B) Apakah bot menganalisa posisi berjalan lalu buka lagi setelah TP/SL?
- Ya, setelah posisi ditutup (`PositionClosed`), simbol akan keluar dari daftar open dan sinyal berikutnya bisa diproses lagi.
- Jadi perilaku re-entry setelah TP/SL **memang memungkinkan** dan normal pada scalper, selama guard duplikasi bekerja benar.

### C) Apakah menimpa posisi meningkatkan risiko?
- **Benar.** Jika posisi yang sedang jalan ditambah tanpa desain pyramiding yang terkontrol, margin naik, liquidation distance menyempit (terutama leverage tinggi), dan tail-risk meningkat.

## Rekomendasi prioritas sebelum live

1. Tambahkan **final execution gate**:
   - sebelum `place_order`, cek PositionBook + `exchange.fetch_open_positions(symbol)`;
   - jika ada posisi simbol yang masih terbuka, blok order baru atau hanya izinkan `reduce_only`.

2. Terapkan **single-position mode** eksplisit di config:
   - `position_mode = single_symbol_single_side` dengan hard reject.

3. Tambahkan **reconcile loop periodik** yang lebih agresif:
   - sinkronisasi state lokal vs exchange per beberapa detik, dengan alarm mismatch.

4. Batasi leverage & notional untuk fase live awal:
   - live shadow -> micro size -> scale bertahap berdasarkan stabilitas 2–4 minggu.

5. Tambahkan **post-trade invariant checks**:
   - assert tidak ada >1 posisi terbuka per symbol (kecuali mode hedged sengaja diaktifkan).

## Verdict operasional
- **Paper trading / sandbox:** cukup siap untuk iterasi.
- **Live trading leverage tinggi:** **belum**; perlu hard gate final anti-stacking + observability posisi exchange yang lebih ketat.
