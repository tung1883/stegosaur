<p align="center">
  <img src="assets/stegosaur.png" width="170" alt="Stegosaur logo: a small green stegosaurus">
</p>

<h1 align="center">Stegosaur</h1>

<p align="center">Hide files inside images, audio and text. Protect them with a passphrase.</p>

---

## 1 Description

Stegosaur hides a secret file inside another file. The other file is the cover. Stegosaur encrypts the secret first. The cover looks the same to a person.

Stegosaur has four programs:

| Program | Name | Use |
|---|---|---|
| Command line | `stegosaur` | Scripts and automation |
| Terminal app | `stegosaur-tui` | Menus in a terminal |
| Desktop app | `stegosaur-desktop` | Windows, with a picture preview |
| Library | `stegosaur` (Rust crate) | Your own programs |

All four programs use the same code. A file made by one program opens in the other programs.

### 1.1 Supported covers

| Type | Formats |
|---|---|
| Image | PNG, BMP, TIFF, WebP (lossless only), PPM, JPEG |
| Audio | WAV (16-bit), FLAC |
| Text | Generated English-like text |

> **NOTE:** Stegosaur does not accept lossy formats such as MP3 or HEIC. A lossy format changes the data and destroys the secret.

### 1.2 Features

- **One file, one passphrase.** Hide one file. Open it with one passphrase.
- **Hidden layout.** Hide two files with two passphrases. Each passphrase opens only its own file. Unused space holds random data. A file with one secret, two secrets or no secret looks the same.
- **Split and join.** Split one secret into N shares. Hide each share in a different cover. Any K shares rebuild the secret. Fewer than K shares give no information.
- **Cover text.** Turn a secret into text that reads like a log file or other plain English-like text.
- **Bulk recovery.** Give Stegosaur a folder and a list of passphrases. Stegosaur finds and recovers all secrets.
- **Keyed file names.** Stegosaur names each new file like a camera photo, for example `IMG_482193.png`. The desktop and terminal apps do this for you. In the command line, use `stegosaur tag-name`. Only your passphrase can show which files are yours.
- **JPEG support.** Stegosaur changes the compressed coefficients of the JPEG. It keeps a progressive JPEG progressive.
- **Detectability check.** Stegosaur measures how easy it is to find your secret. The desktop app shows this before you save.

### 1.3 How it works

1. Stegosaur makes a key from your passphrase with scrypt.
2. Stegosaur encrypts the secret with ChaCha20-Poly1305.
3. Stegosaur adds Reed-Solomon error-correction bytes.
4. Stegosaur chooses the places to change from the key. The places look random.
5. Stegosaur changes the least significant bits of the cover with LSB matching (plus or minus 1).
6. For JPEG, Stegosaur uses a syndrome-trellis code. The code changes the fewest and cheapest coefficients.

A wrong passphrase gives the same result as a file with no secret. This is intended.

---

## 2 Installation

### 2.1 Requirements

- Rust 1.85 or later (install from <https://rustup.rs>)
- Windows only, desktop app: Microsoft WebView2 (included in Windows 11)

### 2.2 Build

1. Download the source code.
2. Go to the project folder.
3. Do the command for the program you need.

```
cargo build --release                       # command line
cargo build --release -p stegosaur-tui      # terminal app
cargo build --release -p stegosaur-desktop  # desktop app
```

The programs are in `target/release/`.

---

## 3 Operation

### 3.1 Hide a file

1. Do this command:

   ```
   stegosaur embed photo.png out.png --data secret.pdf
   ```

2. Enter the passphrase when Stegosaur asks.

> **CAUTION:** Do not put the passphrase on the command line with `--pass`. Other users can see it in the process list. Set the variable `STEGOSAUR_PASS` instead.

### 3.2 Recover a file

```
stegosaur extract out.png --out recovered.pdf
```

### 3.3 Hide two files with two passphrases

```
stegosaur hidden-embed photo.png out.png \
  --public-data first.pdf --public-pass "first phrase" \
  --hidden-data second.pdf --hidden-pass "second phrase"
stegosaur hidden-extract out.png --out result.pdf --pass "second phrase"
```

### 3.4 Split a secret

1. Split the secret across 3 covers. Any 2 covers recover it.

   ```
   stegosaur split --secret key.bin --k 2 --out-dir shares/ a.png b.png c.png
   ```

2. The share files keep the names of the covers. To recover, give any 2 of them to `join`.

   ```
   stegosaur join --out key.bin shares/a.png shares/c.png
   ```

### 3.5 Recover everything in a folder

1. Write each passphrase on its own line in `passphrases.txt`.
2. Do this command:

   ```
   stegosaur recover my-folder --pass-file passphrases.txt
   ```

3. Read the table. Find the recovered files in `my-folder/recovered/`.

Stegosaur does not overwrite files. The `--recursive` option includes sub-folders. The `--names-only` option is faster. It only tries a passphrase on files whose name carries its tag.

### 3.6 Check the detectability of a file

```
stegosaur analyze out.png
```

### 3.7 Use the desktop app

1. Start `stegosaur-desktop`.
2. Click a tab: **Hide**, **Reveal**, **Analyze**, **Split / Join** or **Bulk**.
3. Click files in the list on the left.
4. Set the options on the right.
5. Click the button at the bottom.

---

## 4 Performance

Test computer: AMD Ryzen 5 5600H, release build. Payload: half of the capacity.

| Image size | Capacity | Hide | Recover | Detector scan |
|---|---|---|---|---|
| 1 MP | 343 KB | 106 ms | 93 ms | 10 ms |
| 4 MP | 1.4 MB | 307 ms | 255 ms | 39 ms |
| 8 MP | 2.7 MB | 573 ms | 465 ms | 74 ms |
| 24 MP | 8.2 MB | 2.1 s | 1.7 s | 227 ms |

- Key derivation (scrypt) takes about 53 ms for each file.
- The Hidden layout on an 8 MP image takes 171 ms to hide and 80 ms to recover.
- A 24 MP JPEG takes about 300 ms for each step: read, hide, write, recover.
- Bulk recovery of 12 files with 5 passphrases takes 1.7 s.

To repeat the test, do `cargo run --release --example bench`.

---

## 5 Comparison with other tools

| Tool | Covers | Encryption | Hidden second file | Split secrets | Detectability check |
|---|---|---|---|---|---|
| **Stegosaur** | PNG, BMP, TIFF, WebP, PPM, JPEG, WAV, FLAC, text | ChaCha20-Poly1305, scrypt | Yes | Yes | Yes |
| Steghide | JPEG, BMP, WAV, AU | Yes | No | No | No |
| OpenStego | PNG, BMP and others | Optional | No | No | No |
| OutGuess | JPEG, PNM | No | No | No | No |
| F5 | JPEG | Weak | No | No | No |
| zsteg, StegSolve | PNG, BMP | Not applicable | Not applicable | Not applicable | Detection only |

This table comes from the public documentation of each tool. Check the current version of a tool before you rely on it.

Differences that matter:

- Steghide is old. The tool Stegseek can find weak Steghide passphrases fast.
- Stegosaur uses a memory-hard key derivation. This makes passphrase guessing slow.
- Stegosaur uses a modern distortion-minimising method for JPEG at small payloads. At 10% of the capacity it changes about half as many coefficients as plain matching. The cost of the changes is about 50 times less.

---

## 6 Limits

> **WARNING:** Stegosaur does not make a file safe from every detector. Use a low score as a guide only.

- The built-in detectors are classical (chi-square, RS, SPA, and a calibrated-histogram test for JPEG). A modern machine-learning detector can find more.
- The JPEG detector cannot see payloads below about 20% of the capacity.
- Nobody has audited Stegosaur from outside. Do not use it for data where a failure can cause harm.
- Do not resize, re-save or convert a file that holds a secret. Many chat and photo services recompress images. A recompressed file loses the secret.
- Do not lose the passphrase. Nobody can recover it.
- Use a cover that you never published. A public photo lets an attacker compare it with your file.
- Bulk recovery cannot tell a file with no secret from a file with a secret and a missing passphrase.

---

## 7 Tests

Do `cargo test --workspace --release`. The project has 207 tests.

## 8 Licence

Stegosaur uses two licences. You can choose one of them:

- [MIT](LICENSE-MIT)
- [Apache-2.0](LICENSE-APACHE)
