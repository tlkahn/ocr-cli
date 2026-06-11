# ocr-cli

A local-first OCR pipeline that replaces a chain of fish scripts and Python tools with a single Rust binary. Sends PDFs inline (base64) to Mistral OCR, eliminating the need for S3/Lambda/AWS infrastructure.

## How it works

```
ocr-cli paper.pdf --lead 2 --trail 3
         │
         ├─ 1. Truncate pages ─────────── lmpdf (Pdfium bindings)
         ├─ 2. Extract title ──────────── lmpdf + llm-rs (OpenAI)
         ├─ 3. Mistral OCR ───────────── reqwest + base64 (FileChunk POST)
         ├─ 4. Post-process ──────────── Rust port of mistral-postproc
         └─ 5. Move outputs ──────────── vault + archive
```

## Usage

```sh
ocr-cli paper.pdf
ocr-cli paper.pdf --lead 2 --trail 3
ocr-cli *.pdf --dry-run
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `<files...>` | required | PDF files to process |
| `--lead N` | `0` | Pages to remove from the front |
| `--trail N` | `0` | Pages to remove from the end |
| `--vault PATH` | `~/Documents/Ekuro/` | Output directory for markdown notes |
| `--papers PATH` | `~/Documents/Papers/` | Archive directory for source PDFs |
| `--model MODEL` | `gpt-4o-mini` | LLM model for title extraction |
| `--dry-run` | off | Stop after title extraction, print proposed filename |
| `--verbose` | off | Print detailed progress |

## Requirements

- [Pdfium](https://pdfium.googlesource.com/pdfium/) shared library (searched at `/opt/homebrew/lib/libpdfium.dylib` or via `PDFIUM_PATH`)
- `MISTRAL_API_KEY` environment variable
- `OPENAI_API_KEY` environment variable

## Local dependencies

This project uses two sibling Rust crates via path dependencies:

- [`lmpdf`](../lmpdf) — Pdfium bindings for PDF manipulation (truncation, text extraction)
- [`llm-rs`](../llm-rs) — LLM provider abstraction for title extraction

## Building

```sh
cargo build --release
```

## License

MIT
