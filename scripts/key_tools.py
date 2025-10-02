#!/usr/bin/env python3
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "cryptography",
#     "pydantic",
#     "rich",
#     "rich-argparse",
#     "scalecodec",
#     "substrate-interface",
# ]
# ///
"""
Key utilities for Modnet:
- Generate Aura (sr25519) and GRANDPA (ed25519) keys via `subkey`.
- Inspect/convert public keys to SS58 addresses.
- Derive multisig address (2-of-3, or any threshold) using substrate-interface.

Requirements:
- subkey installed and on PATH (from Substrate).
- Python deps (for multisig): see scripts/requirements.txt

Usage examples:
  # Generate fresh Aura and GRANDPA keypairs
  ./scripts/key_tools.py gen-all --network substrate

  # Generate Aura only
  ./scripts/key_tools.py gen --scheme sr25519 --network substrate

  # Generate GRANDPA only
  ./scripts/key_tools.py gen --scheme ed25519 --network substrate

  # Inspect a public key to SS58
  ./scripts/key_tools.py inspect --public 0x<hex> --network substrate --scheme sr25519

  # Compute multisig address (2-of-3)
  ./scripts/key_tools.py multisig --threshold 2 \
    --signer 5F3sa2TJ... --signer 5DAAnrj7... --signer 5H3K8Z... \
    --ss58-prefix 42

  # Derive public SS58 address from secret phrase
  ./scripts/key_tools.py derive --phrase <phrase> --scheme sr25519 --network substrate

  # Save key to file (encrypted). If --out is omitted, saves to ~/.modnet/keys/<timestamp>-<scheme>.json
  ./scripts/key_tools.py key-save --scheme sr25519 --network substrate --phrase <phrase> --prompt
  ./scripts/key_tools.py key-save --scheme sr25519 --network substrate --phrase <phrase> --out /tmp/key.json --password <password>

  # Load key from file (encrypted)
  ./scripts/key_tools.py key-load --file /tmp/key.json --password <password>
"""
import argparse
import json
import shutil
import subprocess
import sys

from rich.console import Console
from rich.json import JSON
from rich_argparse import RichHelpFormatter
from pydantic import BaseModel, Field, field_validator, ConfigDict

console = Console()

# -----------------------------
# Key object + crypto helpers
# -----------------------------
from getpass import getpass
import os
import base64
from typing import Literal
from datetime import datetime, UTC
import unicodedata
import sys as _sys
import termios
import tty

try:
    from cryptography.hazmat.primitives.kdf.scrypt import Scrypt
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    _CRYPTO_OK = True
except Exception:
    _CRYPTO_OK = False

DEFAULT_KEYS_DIR = os.path.expanduser("~/.modnet/keys")

def ensure_keys_dir() -> None:
    """Create the default keys directory if it doesn't already exist."""
    os.makedirs(DEFAULT_KEYS_DIR, exist_ok=True)


def resolve_key_path(path_or_name: str) -> str:
    """Resolve a key file path.

    If `path_or_name` is an absolute path or contains a path separator, expand and return as-is.
    Otherwise, look for it under DEFAULT_KEYS_DIR, appending .json if needed.
    """
    p = os.path.expanduser(path_or_name)
    if os.path.isabs(p) or os.sep in path_or_name:
        return p
    ensure_keys_dir()
    if not path_or_name.endswith(".json"):
        path_or_name += ".json"
    return os.path.join(DEFAULT_KEYS_DIR, path_or_name)


def list_key_files() -> list[str]:
    """Return a sorted list of key file paths in DEFAULT_KEYS_DIR (most recent first)."""
    ensure_keys_dir()
    try:
        entries = [
            os.path.join(DEFAULT_KEYS_DIR, f)
            for f in os.listdir(DEFAULT_KEYS_DIR)
            if f.endswith(".json") and os.path.isfile(os.path.join(DEFAULT_KEYS_DIR, f))
        ]
    except FileNotFoundError:
        entries = []
    entries.sort(key=lambda p: os.path.getmtime(p), reverse=True)
    return entries

def _require_crypto():
    """Raise if crypto dependencies are missing."""
    if not _CRYPTO_OK:
        raise RuntimeError("Missing crypto deps. Install with: pip install -r scripts/requirements.txt")

def _to_password_bytes(password: str | bytes) -> bytes:
    """Convert password to bytes with Unicode normalization.

    - If bytes, return as-is.
    - If str, normalize to NFC and encode as UTF-8.
    """
    if isinstance(password, bytes):
        return password
    norm = unicodedata.normalize("NFC", password)
    return norm.encode("utf-8")


def _kdf_scrypt(password: str | bytes, salt: bytes, n: int = 2**14, r: int = 8, p: int = 1, length: int = 32) -> bytes:
    """Derive a symmetric key from `password` and `salt` using scrypt."""
    kdf = Scrypt(salt=salt, length=length, n=n, r=r, p=p)
    return kdf.derive(_to_password_bytes(password))


def _read_password_bytes_interactive(prompt: str) -> bytes:
    """Read password as raw bytes from TTY without echo, independent of locale."""
    fd = _sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    try:
        _sys.stderr.write(prompt)
        _sys.stderr.flush()
        tty.setraw(fd)
        buf = bytearray()
        while True:
            ch = os.read(fd, 1)
            if not ch:
                break
            b = ch[0]
            if b in (10, 13):
                _sys.stderr.write("\n")
                _sys.stderr.flush()
                break
            if b == 3:
                raise KeyboardInterrupt
            if b in (8, 127):
                if buf:
                    buf.pop()
                continue
            buf.extend(ch)
        return bytes(buf)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)


def _aesgcm_encrypt(key: bytes, plaintext: bytes, associated_data: bytes = b"") -> dict:
    """Encrypt `plaintext` using AES-GCM with the provided key and AAD."""
    nonce = os.urandom(12)
    aes = AESGCM(key)
    ciphertext = aes.encrypt(nonce, plaintext, associated_data)
    return {"nonce": base64.b64encode(nonce).decode(), "ciphertext": base64.b64encode(ciphertext).decode()}


def _aesgcm_decrypt(key: bytes, nonce_b64: str, ciphertext_b64: str, associated_data: bytes = b"") -> bytes:
    """Decrypt AES-GCM `ciphertext_b64` with `key` and return plaintext bytes."""
    nonce = base64.b64decode(nonce_b64)
    ciphertext = base64.b64decode(ciphertext_b64)
    aes = AESGCM(key)
    return aes.decrypt(nonce, ciphertext, associated_data)


def _json_default(obj):
    """Helper for json.dumps to serialize non-JSON types like datetime."""
    if isinstance(obj, datetime):
        return obj.isoformat()
    raise TypeError(f"Object of type {type(obj).__name__} is not JSON serializable")


class Key(BaseModel):
    model_config = ConfigDict(frozen=False)
    scheme: str = Field(description="Key scheme: sr25519 or ed25519")
    network: str = Field(default="substrate", description="Substrate network id for subkey")
    byte_array: bytes | None = Field(default=None, description="Raw key bytes (if available)")
    mnemonic_phrase: str | None = Field(default=None, description="BIP39 mnemonic (alias)")
    secret_phrase: str | None = Field(default=None, description="Secret phrase / mnemonic")
    public_key_hex: str | None = Field(default=None, description="0x-prefixed public key hex")
    private_key_hex: str | None = Field(default=None, description="0x-prefixed private key hex (if available)")
    ss58_address: str | None = Field(default=None, description="Derived SS58 address")
    key_type: Literal["sr25519", "ed25519", "ss58"] | None = None
    is_pair: bool = False
    is_multisig: bool = False
    threshold: int | None = None
    signers: list[str] | None = None
    multisig_address: str | None = None
    created_at: datetime | None = None

    @field_validator("scheme")
    @classmethod
    def _validate_scheme(cls, scheme_value: str) -> str:
        if scheme_value not in {"sr25519", "ed25519"}:
            raise ValueError("scheme must be 'sr25519' or 'ed25519'")
        return scheme_value

    @staticmethod
    def from_secret_phrase(phrase: str, scheme: str, network: str = "substrate") -> "Key":
        """Build a specific Key subclass from a secret phrase via `subkey inspect`."""
        require_subkey()
        # subkey inspect will print public + ss58 for the phrase
        out = run(["subkey", "inspect", "--scheme", scheme, "--network", network, phrase])
        parsed = parse_subkey_generate(out)
        base_kwargs = dict(
            scheme=scheme,
            network=network,
            secret_phrase=phrase,
            public_key_hex=parsed.get("public_key_hex"),
            private_key_hex=parsed.get("secret_seed"),
            ss58_address=parsed.get("ss58_address"),
            byte_array=ss58_to_bytes(parsed.get("ss58_address")),
            key_type=scheme,
            is_pair=True,
            created_at=datetime.now(UTC),
        )
        if scheme == "sr25519":
            return AuraSR25519Key(**base_kwargs)
        else:
            return GrandpaED25519Key(**base_kwargs)

    @staticmethod
    def from_public(public_hex: str, scheme: str, network: str = "substrate") -> "Key":
        """Build a ModNetSS58Key from a 0x-prefixed public key hex by deriving SS58 via `subkey`."""
        parsed = subkey_inspect(public_hex, network, scheme)
        return ModNetSS58Key(
            scheme=scheme,
            network=network,
            public_key_hex=public_hex,
            ss58_address=parsed.get("ss58_address"),
            byte_array=ss58_to_bytes(parsed.get("ss58_address")),
            key_type="ss58" if parsed.get("ss58_address") else scheme,
            is_pair=False,
            created_at=datetime.now(UTC),
        )

    def derive_public_ss58(self) -> "Key":
        """Ensure `public_key_hex` and `ss58_address` are set, deriving from phrase if needed."""
        if self.public_key_hex and self.ss58_address:
            return self
        if self.secret_phrase:
            derived = Key.from_secret_phrase(self.secret_phrase, self.scheme, self.network)
            self.public_key_hex = derived.public_key_hex
            self.ss58_address = derived.ss58_address
            # populate byte_array from derived ss58
            self.byte_array = ss58_to_bytes(self.ss58_address)
            # populate private key if available from subkey inspect
            if not self.private_key_hex and derived.private_key_hex:
                self.private_key_hex = derived.private_key_hex
            return self
        raise ValueError("No data available to derive from; provide secret_phrase or public_key_hex")

    def to_json(self, include_secret: bool = False) -> dict:
        """Return a JSON-serializable dict of this key. Optionally include the secret.

        Uses Pydantic's JSON mode so datetimes are converted to ISO strings.
        """
        data = self.model_dump(mode="python")
        if not include_secret:
            data.pop("secret_phrase", None)
            data.pop("private_key_hex", None)
        # Ensure byte_array is JSON-safe (hex string) rather than raw bytes
        byte_arr = data.get("byte_array")
        if isinstance(byte_arr, (bytes, bytearray)):
            data["byte_array"] = "0x" + bytes(byte_arr).hex()
        return data

    # Encryption format: JSON with scrypt params, salt, nonce, ciphertext (base64)
    def encrypt(self, password: str | bytes) -> dict:
        """Encrypt this key with the given `password` and return an encrypted JSON blob."""
        _require_crypto()
        payload = json.dumps(self.to_json(include_secret=True), default=_json_default).encode()
        salt = os.urandom(16)
        key = _kdf_scrypt(password, salt)
        enc = _aesgcm_encrypt(key, payload)
        return {
            "version": 1,
            "kdf": "scrypt",
            "salt": base64.b64encode(salt).decode(),
            "params": {"n": 16384, "r": 8, "p": 1},
            **enc,
        }

    @staticmethod
    def decrypt(encrypted_blob: dict, password: str | bytes) -> "Key":
        """Decrypt a previously saved key blob using `password` and reconstruct a Key."""
        _require_crypto()
        if encrypted_blob.get("kdf") != "scrypt":
            raise ValueError("Unsupported KDF")
        params = encrypted_blob.get("params") or {}
        n, r, p = params.get("n", 16384), params.get("r", 8), params.get("p", 1)
        salt = base64.b64decode(encrypted_blob["salt"]) if isinstance(encrypted_blob.get("salt"), str) else encrypted_blob.get("salt")
        key = _kdf_scrypt(password, salt, n=n, r=r, p=p)
        plaintext_bytes = _aesgcm_decrypt(key, encrypted_blob["nonce"], encrypted_blob["ciphertext"])  # type: ignore
        decrypted_data = json.loads(plaintext_bytes.decode())
        # Convert byte_array back from hex string if present
        byte_array_value = decrypted_data.get("byte_array")
        if isinstance(byte_array_value, str) and byte_array_value.startswith("0x"):
            try:
                decrypted_data["byte_array"] = bytes.fromhex(byte_array_value[2:])
            except ValueError:
                decrypted_data["byte_array"] = None
        return Key(
            scheme=decrypted_data["scheme"],
            network=decrypted_data.get("network", "substrate"),
            secret_phrase=decrypted_data.get("secret_phrase"),
            public_key_hex=decrypted_data.get("public_key_hex"),
            private_key_hex=decrypted_data.get("private_key_hex"),
            ss58_address=decrypted_data.get("ss58_address"),
            byte_array=decrypted_data.get("byte_array"),
            key_type=decrypted_data.get("key_type"),
            is_pair=decrypted_data.get("is_pair", False),
            is_multisig=decrypted_data.get("is_multisig", False),
            threshold=decrypted_data.get("threshold"),
            signers=decrypted_data.get("signers"),
            multisig_address=decrypted_data.get("multisig_address"),
            created_at=datetime.now(UTC),
        )

    def save(self, path: str, password: str | bytes | None = None) -> None:
        """Encrypt and write the key to `path`. If no password is provided, prompt securely."""
        # Best-effort enrichment prior to save: ensure we persist private_key_hex and byte_array
        try:
            if self.secret_phrase and not self.private_key_hex:
                derived = Key.from_secret_phrase(self.secret_phrase, self.scheme, self.network)
                if derived.private_key_hex and not self.private_key_hex:
                    self.private_key_hex = derived.private_key_hex
                if derived.ss58_address and not self.ss58_address:
                    self.ss58_address = derived.ss58_address
                if not self.byte_array:
                    self.byte_array = ss58_to_bytes(self.ss58_address)
            elif self.ss58_address and not self.byte_array:
                self.byte_array = ss58_to_bytes(self.ss58_address)
        except Exception:
            # Do not block saving if enrichment fails
            pass
        if password is None:
            console.print("[cyan]Hit Ctrl-C to exit[/cyan]")
            pw1 = _read_password_bytes_interactive("Set password for key file: ")
            pw2 = _read_password_bytes_interactive("Confirm password: ")
            if pw1 != pw2:
                raise ValueError("Passwords do not match")
            if len(pw1) == 0:
                raise ValueError("Password cannot be empty")
            password = pw1
        blob = self.encrypt(password)
        # Ensure the parent directory exists
        parent = os.path.dirname(os.path.expanduser(path)) or "."
        os.makedirs(parent, exist_ok=True)
        with open(os.path.expanduser(path), "w") as file:
            json.dump(blob, file, indent=2)

    @staticmethod
    def load(path: str, password: str | bytes | None = None) -> "Key":
        """Load and decrypt a key from `path`. If no password is provided, prompt securely."""
        if password is None:
            password = _read_password_bytes_interactive("Password for key file: ")
        with open(os.path.expanduser(path), "r") as file:
            encrypted_blob = json.load(file)
        return Key.decrypt(encrypted_blob, password)


class AuraSR25519Key(Key):
    secret_phrase: str


class GrandpaED25519Key(Key):
    secret_phrase: str


class ModNetSS58Key(Key):
    public_key_hex: str
    ss58_address: str


def subkey_inspect(public_hex: str, network: str, scheme: str) -> dict:
    """Call `subkey inspect` to derive SS58 address info for a public key."""
    require_subkey()
    # subkey inspect --network substrate --public --scheme sr25519 0x<hex>
    out = run(["subkey", "inspect", "--network", network, "--public", "--scheme", scheme, public_hex])
    return parse_subkey_generate(out)


def ss58_to_bytes(address: str | None) -> bytes | None:
    """Decode an SS58 address to its 32-byte AccountId, returning bytes or None on failure."""
    if not address:
        return None
    try:
        from substrateinterface.utils.ss58 import ss58_decode
        hex_str = ss58_decode(address)
        return bytes.fromhex(hex_str)
    except Exception:
        return None

def multisig_address(signers: list[str], threshold: int, ss58_prefix: int) -> dict:
    """Compute a deterministic pallet-multisig address from signers and threshold."""
    try:
        from substrateinterface.utils.ss58 import ss58_encode, ss58_decode
        from hashlib import blake2b
    except Exception as e:
        sys.stderr.write("Error: Python deps missing. Install from scripts/requirements.txt\n")
        raise

    # The multisig account id in pallet-multisig is constructed deterministically from sorted signers and threshold.
    # Reference (pallet-multisig): multi_account_id = AccountId::from(blake2_256(b"modlpy/utilisig" ++ sorted_signers ++ threshold LE));
    # We implement the same here to ensure exact match.
    tag = b"modlpy/utilisig"

    # Decode SS58 to raw pubkey bytes (AccountId32)
    signer_pubkeys = [bytes.fromhex(ss58_decode(signer)) for signer in signers]
    # Sort lexicographically as per pallet
    signer_pubkeys.sort()

    # threshold as little endian u16
    threshold_le = threshold.to_bytes(2, byteorder="little")

    hasher = blake2b(digest_size=32)
    hasher.update(tag)
    for public_key in signer_pubkeys:
        hasher.update(public_key)
    hasher.update(threshold_le)
    account_id = hasher.digest()

    address = ss58_encode(account_id.hex(), ss58_format=ss58_prefix)
    return {"account_id_hex": account_id.hex(), "ss58_address": address}


def _print_json(data_obj: dict):
    """Pretty-print JSON robustly, converting datetimes via _json_default.

    - If stdout is a TTY, render pretty using Rich from pre-serialized JSON text.
    - Otherwise, emit raw JSON text.
    """
    json_text = json.dumps(data_obj, indent=2, default=_json_default)
    if sys.stdout.isatty():
        console.print(json_text)
    else:
        sys.stdout.write(json_text + "\n")


def _default_out_path(scheme: str, role_hint: str | None = None) -> str:
    """Compute a default output path in DEFAULT_KEYS_DIR with timestamp and scheme.

    If role_hint is provided (e.g., "aura" or "grandpa"), include it in the filename.
    """
    ensure_keys_dir()
    timestamp_str = datetime.now(UTC).strftime("%Y%m%d-%H%M%S")
    if role_hint:
        filename = f"{timestamp_str}-{role_hint}-{scheme}.json"
    else:
        filename = f"{timestamp_str}-{scheme}.json"
    return os.path.join(DEFAULT_KEYS_DIR, filename)


def _save_key_with_password(key_obj: "Key", out_path: str | None, scheme: str, password: str | bytes | None, role_hint: str | None = None) -> str:
    """Save key_obj to out_path (or default), using provided password (non-interactive) or prompting if None."""
    target_path = os.path.expanduser(out_path) if out_path else _default_out_path(scheme, role_hint)
    key_obj.save(target_path, password)
    return target_path


def _read_password_sources(password: str | None, password_file: str | None, password_stdin: bool, prompt: bool, prompt_set: str, prompt_confirm: str, for_load: bool = False) -> str | bytes | None:
    """Obtain password from: direct arg, file, stdin, or interactive prompt."""
    if password is not None:
        return password
    if password_file:
        with open(os.path.expanduser(password_file), "rb") as f:
            return f.read().rstrip(b"\r\n")
    if password_stdin:
        return sys.stdin.buffer.readline().rstrip(b"\r\n")
    if prompt:
        if for_load:
            return _read_password_bytes_interactive(prompt_set)
        else:
            pw1 = _read_password_bytes_interactive(prompt_set)
            pw2 = _read_password_bytes_interactive(prompt_confirm)
            if pw1 != pw2:
                raise ValueError("Passwords do not match")
            return pw1
    return None


def cmd_gen(args):
    """Handle `gen` subcommand: generate a single keypair and save it encrypted."""
    key_obj = subkey_generate(args.scheme, args.network)
    # Determine output path: prefer --out full path; else --name under default dir; else timestamped default
    if args.out:
        out_path = os.path.expanduser(args.out)
    else:
        ensure_keys_dir()
        if getattr(args, "name", None):
            filename = args.name
            if not filename.endswith(".json"):
                filename += ".json"
            out_path = os.path.join(DEFAULT_KEYS_DIR, filename)
        else:
            out_path = _default_out_path(args.scheme, None)
    pw = _read_password_sources(args.password, getattr(args, "password_file", None), getattr(args, "password_stdin", False), True, "Set password for key file: ", "Confirm password: ")
    key_obj.save(out_path, pw)
    console.print(f"[green]Saved generated key to[/green] {out_path}")
    _print_json(key_obj.to_json(include_secret=False))


def cmd_gen_all(args):
    """Handle `gen-all` subcommand: generate Aura and GRANDPA keypairs and save them encrypted."""
    aura_key = subkey_generate("sr25519", args.network)
    grandpa_key = subkey_generate("ed25519", args.network)
    # Determine out directory
    out_dir = os.path.expanduser(args.out_dir) if getattr(args, "out_dir", None) else DEFAULT_KEYS_DIR
    os.makedirs(out_dir, exist_ok=True)
    timestamp_str = datetime.now(UTC).strftime("%Y%m%d-%H%M%S")
    # Filenames: allow user-provided names, else timestamp defaults
    if getattr(args, "aura_name", None):
        aura_base = args.aura_name
        if not aura_base.endswith(".json"):
            aura_base += ".json"
    else:
        aura_base = f"{timestamp_str}-aura-sr25519.json"
    if getattr(args, "grandpa_name", None):
        grandpa_base = args.grandpa_name
        if not grandpa_base.endswith(".json"):
            grandpa_base += ".json"
    else:
        grandpa_base = f"{timestamp_str}-grandpa-ed25519.json"
    aura_filename = os.path.join(out_dir, aura_base)
    grandpa_filename = os.path.join(out_dir, grandpa_base)
    pw = _read_password_sources(args.password, getattr(args, "password_file", None), getattr(args, "password_stdin", False), True, "Set password for key files: ", "Confirm password: ")
    aura_key.save(aura_filename, pw)
    grandpa_key.save(grandpa_filename, pw)
    console.print(f"[green]Saved Aura key to[/green] {aura_filename}")
    console.print(f"[green]Saved GRANDPA key to[/green] {grandpa_filename}")
    _print_json({
        "aura": aura_key.to_json(include_secret=False),
        "grandpa": grandpa_key.to_json(include_secret=False),
        "network": args.network,
        "saved": {"aura": aura_filename, "grandpa": grandpa_filename},
    })


def cmd_inspect(args):
    """Handle `inspect` subcommand: map a public key to SS58 address."""
    key_obj = Key.from_public(args.public, args.scheme, args.network)
    _print_json(key_obj.to_json(include_secret=False))


def cmd_multisig(args):
    """Handle `multisig` subcommand: compute multisig account from signers/threshold."""
    result = multisig_address(args.signer, args.threshold, args.ss58_prefix)
    _print_json({"threshold": args.threshold, "ss58_prefix": args.ss58_prefix, **result, "signers": args.signer})


def cmd_derive(args):
    """Handle `derive` subcommand: derive public/SS58 from phrase or public key."""
    if args.phrase:
        key_obj = Key.from_secret_phrase(args.phrase, args.scheme, args.network)
    elif args.public:
        key_obj = Key.from_public(args.public, args.scheme, args.network)
    else:
        raise ValueError("Provide --phrase or --public")
    key_obj = key_obj.derive_public_ss58()
    _print_json(key_obj.to_json(include_secret=args.with_secret))


def cmd_key_save(args):
    """Handle `key-save` subcommand: encrypt a key and save it to disk."""
    phrase: str | None = None
    if getattr(args, "phrase_prompt", False) or (not args.phrase and not args.public):
        # Securely prompt for the phrase (hidden input). Do not echo to terminal/history.
        phrase = getpass("Enter secret phrase (input hidden): ")
        if not phrase:
            raise ValueError("Secret phrase cannot be empty")
    if args.phrase:
        phrase = args.phrase
    if phrase:
        key_obj = Key.from_secret_phrase(phrase, args.scheme, args.network)
    elif args.public:
        key_obj = Key.from_public(args.public, args.scheme, args.network)
    else:
        raise ValueError("Provide --phrase/--phrase-prompt or --public")
    # Determine output path
    if args.out:
        out_path = os.path.expanduser(args.out)
    else:
        ensure_keys_dir()
        if args.name:
            filename = args.name
            if not filename.endswith(".json"):
                filename += ".json"
        else:
            timestamp_str = datetime.now(UTC).strftime("%Y%m%d-%H%M%S")
            filename = f"{timestamp_str}-{args.scheme}.json"
        out_path = os.path.join(DEFAULT_KEYS_DIR, filename)
    pw = _read_password_sources(args.password, getattr(args, "password_file", None), getattr(args, "password_stdin", False), bool(getattr(args, "prompt", False)), "Set password for key file: ", "Confirm password: ")
    key_obj.save(out_path, pw)
    console.print(f"[green]Saved encrypted key to[/green] {out_path}")


def cmd_key_load(args):
    """Handle `key-load` subcommand: decrypt a key file and print fields."""
    path = resolve_key_path(args.file)
    pw = _read_password_sources(args.password, getattr(args, "password_file", None), getattr(args, "password_stdin", False), bool(getattr(args, "prompt", False)), "Password for key file: ", "", for_load=True)
    key_obj = Key.load(path, pw)
    _print_json(key_obj.to_json(include_secret=args.with_secret))


def cmd_list(args):
    files = list_key_files()
    if not files:
        console.print("[yellow]No key files found in[/yellow] " + DEFAULT_KEYS_DIR)
        return
    rows = [
        {
            "index": i,
            "file": os.path.basename(p),
            "modified": datetime.fromtimestamp(os.path.getmtime(p), UTC).isoformat(),
        }
        for i, p in enumerate(files)
    ]
    _print_json({"keys_dir": DEFAULT_KEYS_DIR, "items": rows})


def cmd_select(args):
    files = list_key_files()
    if not files:
        console.print("[yellow]No key files found in[/yellow] " + DEFAULT_KEYS_DIR)
        return
    if args.index is not None:
        idx = args.index
        if idx < 0 or idx >= len(files):
            raise ValueError(f"Index out of range (0..{len(files)-1})")
        chosen = files[idx]
        interactive = False
    else:
        console.print(f"[cyan]Select a key file from[/cyan] {DEFAULT_KEYS_DIR}:")
        for i, p in enumerate(files):
            console.print(f"  [{i}] {os.path.basename(p)}")
        while True:
            s = input("Enter index: ").strip()
            if s.isdigit():
                idx = int(s)
                if 0 <= idx < len(files):
                    chosen = files[idx]
                    break
            console.print("[red]Invalid selection, try again.[/red]")
        interactive = True
    # Build result and decide whether to show key contents.
    result = {"index": idx, "selected": chosen, "filename": os.path.basename(chosen)}
    do_show = bool(getattr(args, "show", False) or interactive)
    if do_show:
        try:
            # If no password provided, pass None to trigger secure prompt inside Key.load
            pw = args.password if getattr(args, "password", None) else None
            key_obj = Key.load(chosen, pw)
            result["key"] = key_obj.to_json(include_secret=bool(getattr(args, "with_secret", False)))
        except Exception as e:
            result["error"] = f"failed to load: {e}"
    _print_json(result)





def require_subkey():
    """Ensure the `subkey` binary is available on PATH, or exit with an error."""
    if not shutil.which("subkey"):
        sys.stderr.write("Error: 'subkey' not found on PATH. Install Substrate subkey tool.\n")
        sys.exit(1)

def run(cmd: list[str]) -> str:
    """Run a subprocess command and return stdout, raising if non-zero exit."""
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"Command failed: {' '.join(cmd)}\nSTDERR:\n{proc.stderr}")
    return proc.stdout

def parse_subkey_generate(output: str) -> dict:
    """Parse `subkey generate/inspect` output for secret phrase, secret seed, public key, and SS58."""
    # subkey generate --scheme <scheme> prints a well-known format
    # We'll extract: Secret phrase, Public key (hex), SS58 Address
    data = {
        "secret_phrase": None,
        "secret_seed": None,  # 0x-prefixed seed/private key
        "public_key_hex": None,
        "ss58_address": None,
    }
    for line in output.splitlines():
        line = line.strip()
        if line.lower().startswith("secret phrase"):
            # e.g., Secret phrase:      equip will roof ...
            data["secret_phrase"] = line.split(":", 1)[1].strip()
        elif line.lower().startswith("secret seed"):
            # e.g., Secret seed:       0x1234...
            data["secret_seed"] = line.split(":", 1)[1].strip()
        elif line.lower().startswith("public key (hex)"):
            data["public_key_hex"] = line.split(":", 1)[1].strip()
        elif line.lower().startswith("ss58 address"):
            data["ss58_address"] = line.split(":", 1)[1].strip()
    return data

def subkey_generate(scheme: str, network: str) -> Key:
    """Generate a new keypair via `subkey generate` and return a Key object."""
    require_subkey()
    out = run(["subkey", "generate", "--scheme", scheme, "--network", network])
    parsed = parse_subkey_generate(out)
    secret = parsed.get("secret_phrase")
    private_hex = parsed.get("secret_seed")
    ss58_addr = parsed.get("ss58_address")
    acct_bytes = ss58_to_bytes(ss58_addr)
    if scheme == "sr25519":
        return AuraSR25519Key(
            scheme=scheme,
            network=network,
            secret_phrase=secret,
            public_key_hex=parsed.get("public_key_hex"),
            private_key_hex=private_hex,
            ss58_address=ss58_addr,
            byte_array=acct_bytes,
            key_type="sr25519",
            is_pair=True,
            created_at=datetime.now(UTC),
        )
    else:
        return GrandpaED25519Key(
            scheme=scheme,
            network=network,
            secret_phrase=secret,
            public_key_hex=parsed.get("public_key_hex"),
            private_key_hex=private_hex,
            ss58_address=ss58_addr,
            byte_array=acct_bytes,
            key_type="ed25519",
            is_pair=True, 
            created_at=datetime.now(UTC),
        )

class HelpOnErrorParser(argparse.ArgumentParser):
    def error(self, message):
        """Override to show help text along with the error message."""
        console.print(f"[red]Error:[/red] {message}")
        self.print_help()
        self.exit(2)


def main():
    """CLI entrypoint for key utilities."""
    p = HelpOnErrorParser(description="Key tools for Modnet", formatter_class=RichHelpFormatter)
    sub = p.add_subparsers(dest="command")

    p_gen = sub.add_parser("gen", help="Generate a single keypair via subkey")
    p_gen.add_argument("--scheme", choices=["sr25519", "ed25519"], required=True)
    p_gen.add_argument("--network", default="substrate")
    p_gen.add_argument("--out", help="Output file path (default: ~/.modnet/keys/<timestamp>-<scheme>.json)")
    p_gen.add_argument("--name", help="Filename to use under ~/.modnet/keys instead of a timestamp (e.g., aura-validator.json)")
    p_gen.add_argument("--password", help="Encrypt without prompting using this password")
    p_gen.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_gen.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_gen.set_defaults(func=cmd_gen)

    p_gen_all = sub.add_parser("gen-all", help="Generate Aura (sr25519) and GRANDPA (ed25519) keypairs")
    p_gen_all.add_argument("--network", default="substrate")
    p_gen_all.add_argument("--out-dir", help="Directory to save both keys (default: ~/.modnet/keys/)")
    p_gen_all.add_argument("--aura-name", help="Filename for the Aura key (e.g., aura-node-1.json)")
    p_gen_all.add_argument("--grandpa-name", help="Filename for the GRANDPA key (e.g., grandpa-node-1.json)")
    p_gen_all.add_argument("--password", help="Encrypt without prompting using this password for both keys")
    p_gen_all.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_gen_all.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_gen_all.set_defaults(func=cmd_gen_all)

    p_inspect = sub.add_parser("inspect", help="Inspect a public key to SS58 address")
    p_inspect.add_argument("--public", required=True, help="0x<hex public key>")
    p_inspect.add_argument("--scheme", choices=["sr25519", "ed25519"], required=True)
    p_inspect.add_argument("--network", default="substrate")
    p_inspect.set_defaults(func=cmd_inspect)

    p_multi = sub.add_parser("multisig", help="Compute multisig address from signers and threshold")
    p_multi.add_argument("--signer", action="append", required=True, help="SS58 signer address; pass multiple --signer")
    p_multi.add_argument("--threshold", type=int, required=True)
    p_multi.add_argument("--ss58-prefix", type=int, default=42)
    p_multi.set_defaults(func=cmd_multisig)

    p_derive = sub.add_parser("derive", help="Derive public/SS58 from a secret phrase or public key")
    p_derive.add_argument("--scheme", choices=["sr25519", "ed25519"], required=True)
    p_derive.add_argument("--network", default="substrate")
    p_derive.add_argument("--phrase", help="Secret phrase (mnemonic)")
    p_derive.add_argument("--public", help="0x<hex public key>")
    p_derive.add_argument("--with-secret", action="store_true", help="Include secret in output (if available)")
    p_derive.set_defaults(func=cmd_derive)

    p_save = sub.add_parser("key-save", help="Encrypt and save a key file (scrypt+AES-GCM)")
    p_save.add_argument("--scheme", choices=["sr25519", "ed25519"], required=True)
    p_save.add_argument("--network", default="substrate")
    p_save.add_argument("--phrase", help="Secret phrase (mnemonic)")
    p_save.add_argument("--phrase-prompt", action="store_true", help="Prompt securely for the secret phrase (input hidden)")
    p_save.add_argument("--public", help="0x<hex public key>")
    p_save.add_argument("--out", help="Output file path (default: ~/.modnet/keys/<timestamp>-<scheme>.json)")
    p_save.add_argument("--name", help="Filename to use under ~/.modnet/keys (e.g., aura-sr25519.json)")
    p_save.add_argument("--password", help="Password (omit to be prompted)")
    p_save.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_save.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_save.add_argument("--prompt", action="store_true", help="Prompt for password (recommended)")
    p_save.set_defaults(func=cmd_key_save)

    # Short alias: save
    p_save2 = sub.add_parser("save", help="Alias for key-save")
    p_save2.add_argument("--scheme", choices=["sr25519", "ed25519"], required=True)
    p_save2.add_argument("--network", default="substrate")
    p_save2.add_argument("--phrase", help="Secret phrase (mnemonic)")
    p_save2.add_argument("--phrase-prompt", action="store_true", help="Prompt securely for the secret phrase (input hidden)")
    p_save2.add_argument("--public", help="0x<hex public key>")
    p_save2.add_argument("--out", help="Output file path (default: ~/.modnet/keys/<timestamp>-<scheme>.json)")
    p_save2.add_argument("--name", help="Filename to use under ~/.modnet/keys (e.g., aura-sr25519.json)")
    p_save2.add_argument("--password", help="Password (omit to be prompted)")
    p_save2.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_save2.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_save2.add_argument("--prompt", action="store_true", help="Prompt for password (recommended)")
    p_save2.set_defaults(func=cmd_key_save)

    p_load = sub.add_parser("key-load", help="Decrypt a saved key file and print fields")
    p_load.add_argument("--file", required=True, help="Path or filename in ~/.modnet/keys")
    p_load.add_argument("--password", help="Password (omit to be prompted)")
    p_load.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_load.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_load.add_argument("--prompt", action="store_true", help="Prompt for password")
    p_load.add_argument("--with-secret", action="store_true", help="Include secret in output")
    p_load.set_defaults(func=cmd_key_load)

    # Short alias: load
    p_load2 = sub.add_parser("load", help="Alias for key-load")
    p_load2.add_argument("--file", required=True, help="Path or filename in ~/.modnet/keys")
    p_load2.add_argument("--password", help="Password (omit to be prompted)")
    p_load2.add_argument("--password-file", help="Read password bytes from a file (raw, newline trimmed)")
    p_load2.add_argument("--password-stdin", action="store_true", help="Read password bytes from stdin (first line)")
    p_load2.add_argument("--prompt", action="store_true", help="Prompt for password")
    p_load2.add_argument("--with-secret", action="store_true", help="Include secret in output")
    p_load2.set_defaults(func=cmd_key_load)

    p_list = sub.add_parser("list", help="List key files in ~/.modnet/keys")
    p_list.set_defaults(func=cmd_list)

    p_select = sub.add_parser("select", help="Interactively select a key file from ~/.modnet/keys")
    p_select.add_argument("--index", type=int, help="Preselect by index (non-interactive)")
    p_select.add_argument("--show", action="store_true", help="After selecting, decrypt and print the key JSON")
    p_select.add_argument("--with-secret", action="store_true", help="Include secret in output when using --show")
    p_select.add_argument("--password", help="Password for decryption (omit to prompt)")
    p_select.add_argument("--prompt", action="store_true", help="Prompt for password when using --show")
    p_select.set_defaults(func=cmd_select)


    if len(sys.argv) == 1:
        p.print_help()
        sys.exit(2)

    args = p.parse_args()
    if not hasattr(args, "func"):
        p.print_help()
        sys.exit(2)
    try:
        args.func(args)
    except Exception as e:
        console.print(f"[red]Error:[/red] {e}")
        p.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
