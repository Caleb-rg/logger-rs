# logger-rs

## Install Rust
```bash
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Run (Release Mode)
```bash
$ cargo run --release
```

or shorthand:
```bash
$ cargo r -r
```

## Dummy env
```env
DB_USER="username"
DB_PASSWORD="password"
DB_HOST="localhost"
DB_PORT="5432"
DB_NAME="logging"
HOST="localhost"
PORT="8080"
KEY="x"
LIMIT="100" # Optional
```
