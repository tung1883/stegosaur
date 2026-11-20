//! Built-in cover-text styles. Each style is a deterministic synthetic corpus that a model is
//! trained on, so encoder and decoder get identical models without shipping large files.
//!
//! Log and config styles suit ciphertext well: real logs are full of hashes, ports, IDs and tokens,
//! so high-entropy words are normal there. All generated values (IPs, users, keys, passwords) are random
//! placeholders, not real credentials.

use crate::linguistic::{NgramModel, TokenizerKind, DEFAULT_ORDER};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Style {
    /// English sentences.
    Prose,
    /// Linux syslog / auth.log (sshd, sudo, systemd, cron, kernel).
    Syslog,
    /// PostgreSQL server log.
    Postgres,
    /// nginx combined access log.
    Nginx,
    /// `.env`-style config with secret-looking values.
    Env,
}

impl Style {
    pub fn tokenizer(self) -> TokenizerKind {
        if self == Style::Prose { TokenizerKind::Prose } else { TokenizerKind::Lines }
    }

    /// Syslog mixes sshd templates at order 2 (about 75% well-formed lines); order 4 reaches about 95%.
    pub fn default_order(self) -> usize {
        if self == Style::Syslog { 4 } else { DEFAULT_ORDER }
    }

    pub fn default_top_k(self) -> usize {
        if self == Style::Prose { 16 } else { 32 }
    }

    pub fn corpus(self) -> String {
        let mut rng = ChaCha8Rng::seed_from_u64(0x57E6_05A0);
        match self {
            Style::Prose => include_str!("../assets/corpus.txt").to_string(),
            Style::Syslog => syslog(&mut rng, 3000),
            Style::Postgres => postgres(&mut rng, 3000),
            Style::Nginx => nginx(&mut rng, 3000),
            Style::Env => env_file(&mut rng, 90),
        }
    }

    /// Model for this style; `corpus` replaces the built-in training text (the tokenizer still follows the style).
    pub fn model(self, corpus: Option<&str>, top_k: Option<usize>, order: Option<usize>) -> NgramModel {
        let text = corpus.map(str::to_string).unwrap_or_else(|| self.corpus());
        NgramModel::train_ordered(
            &text,
            top_k.unwrap_or(self.default_top_k()),
            self.tokenizer(),
            order.unwrap_or(self.default_order()),
        )
    }
}

type Rng8 = ChaCha8Rng;

fn pick<'a, T>(rng: &mut Rng8, v: &'a [T]) -> &'a T {
    &v[rng.gen_range(0..v.len())]
}

fn pool<T>(n: usize, mut f: impl FnMut() -> T) -> Vec<T> {
    (0..n).map(|_| f()).collect()
}

fn chars(rng: &mut Rng8, alphabet: &[u8], n: usize) -> String {
    (0..n).map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char).collect()
}

fn hex(rng: &mut Rng8, n: usize) -> String {
    chars(rng, b"0123456789abcdef", n)
}

fn b64(rng: &mut Rng8, n: usize) -> String {
    chars(rng, b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/", n)
}

fn word_pw(rng: &mut Rng8, n: usize) -> String {
    chars(rng, b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789", n)
}

fn ipv4(rng: &mut Rng8) -> String {
    match rng.gen_range(0..3) {
        0 => format!("10.{}.{}.{}", rng.gen_range(0..8), rng.gen_range(0..256), rng.gen_range(1..255)),
        1 => format!("192.168.{}.{}", rng.gen_range(0..4), rng.gen_range(2..255)),
        _ => format!("203.0.113.{}", rng.gen_range(1..255)),
    }
}

fn clock(rng: &mut Rng8) -> String {
    format!("{:02}:{:02}:{:02}", rng.gen_range(0..24), rng.gen_range(0..60), rng.gen_range(0..60))
}

const USERS: &[&str] = &["deploy", "admin", "alice", "bob", "carol", "backup", "ubuntu", "postgres", "www-data", "ops", "git", "jenkins"];
const HOSTS: &[&str] = &["web01", "web02", "db01", "app03", "bastion", "cache01"];

fn syslog(rng: &mut Rng8, lines: usize) -> String {
    let times = pool(150, || clock(rng));
    let ips = pool(70, || ipv4(rng));
    let ports = pool(90, || rng.gen_range(1024..65535u32));
    let pids = pool(60, || rng.gen_range(300..32000u32));
    let fps = pool(40, || b64(rng, 43));
    let sessions = pool(40, || rng.gen_range(1..4000u32));
    let uptimes = pool(60, || format!("{}.{:06}", rng.gen_range(10..90000), rng.gen_range(0..1_000_000)));
    let mut out = String::new();
    for _ in 0..lines {
        let ts = format!("Nov {} {}", rng.gen_range(14..20), pick(rng, &times));
        let host = pick(rng, HOSTS);
        let user = pick(rng, USERS);
        let (ip, port, pid) = (pick(rng, &ips).clone(), pick(rng, &ports), pick(rng, &pids));
        let msg = match rng.gen_range(0..14) {
            0 | 1 => format!("sshd[{pid}]: Accepted publickey for {user} from {ip} port {port} ssh2: ED25519 SHA256:{}", pick(rng, &fps)),
            2 | 3 => format!("sshd[{pid}]: Failed password for invalid user {user} from {ip} port {port} ssh2"),
            4 => format!("sshd[{pid}]: Connection closed by {ip} port {port} [preauth]"),
            5 => format!("sshd[{pid}]: pam_unix(sshd:session): session opened for user {user} by (uid=0)"),
            6 => format!("sudo: {user} : TTY=pts/0 ; PWD=/home/{user} ; USER=root ; COMMAND=/usr/bin/systemctl restart nginx"),
            7 => format!("systemd[1]: Started Session {} of user {user}.", pick(rng, &sessions)),
            8 => "systemd[1]: Starting Daily apt download activities...".to_string(),
            9 => "systemd[1]: Reached target Timers.".to_string(),
            10 => format!("CRON[{pid}]: (root) CMD (cd / && run-parts --report /etc/cron.hourly)"),
            11 => format!("kernel: [{}] EXT4-fs (sda1): re-mounted. Opts: errors=remount-ro", pick(rng, &uptimes)),
            12 => format!("dhclient[{pid}]: DHCPREQUEST for {ip} on eth0 to 10.0.0.1 port 67"),
            _ => format!("systemd-logind[{pid}]: Removed session {}.", pick(rng, &sessions)),
        };
        out.push_str(&format!("{ts} {host} {msg}\n"));
    }
    out
}

fn postgres(rng: &mut Rng8, lines: usize) -> String {
    let times = pool(150, || format!("2025-11-{} {}.{:03} UTC", rng.gen_range(14..20), clock(rng), rng.gen_range(0..1000)));
    let pids = pool(40, || rng.gen_range(300..32000u32));
    let ips = pool(50, || ipv4(rng));
    let ids = pool(80, || rng.gen_range(1..99999u32));
    let ms = pool(80, || format!("{}.{:03}", rng.gen_range(0..900), rng.gen_range(0..1000)));
    let dbs = ["orders", "billing", "analytics", "app"];
    let tables = ["users", "orders", "events", "sessions", "invoices"];
    let mut out = String::new();
    for _ in 0..lines {
        let ts = pick(rng, &times).clone();
        let pid = pick(rng, &pids);
        let (user, db) = (pick(rng, USERS), pick(rng, &dbs));
        let id = pick(rng, &ids);
        let tail = match rng.gen_range(0..12) {
            0 | 1 => format!("LOG:  connection authorized: user={user} database={db} SSL enabled (protocol=TLSv1.3, cipher=TLS_AES_256_GCM_SHA384, bits=256)"),
            2 => "LOG:  checkpoint starting: time".to_string(),
            3 => format!(
                "LOG:  checkpoint complete: wrote {} buffers ({}.{}%); 0 WAL file(s) added, 0 removed, {} recycled; write={} s, sync=0.00{} s, total={} s; distance={} kB",
                pick(rng, &ids), rng.gen_range(0..10), rng.gen_range(0..10), rng.gen_range(0..4), pick(rng, &ms), rng.gen_range(0..9), pick(rng, &ms), pick(rng, &ids)
            ),
            4 | 5 => format!("LOG:  duration: {} ms  statement: SELECT id, email FROM {} WHERE id = {id};", pick(rng, &ms), pick(rng, &tables)),
            6 => format!("ERROR:  duplicate key value violates unique constraint \"{}_email_key\"", pick(rng, &tables)),
            7 => format!("DETAIL:  Key (email)=(user{id}@example.com) already exists."),
            8 => format!("LOG:  automatic vacuum of table \"{db}.public.{}\": index scans: 1", pick(rng, &tables)),
            9 => "LOG:  could not receive data from client: Connection reset by peer".to_string(),
            10 => format!("FATAL:  password authentication failed for user \"{user}\""),
            _ => format!("DETAIL:  Connection matched pg_hba.conf line {}: \"host all all {}/32 scram-sha-256\"", rng.gen_range(80..120), pick(rng, &ips)),
        };
        out.push_str(&format!("{ts} [{pid}] {user}@{db} {tail}\n"));
    }
    out
}

fn nginx(rng: &mut Rng8, lines: usize) -> String {
    let ips = pool(80, || ipv4(rng));
    let stamps = pool(150, || format!("[{}/Nov/2025:{} +0000]", rng.gen_range(14..20), clock(rng)));
    let ids = pool(60, || rng.gen_range(1..9999u32));
    let sizes = pool(80, || rng.gen_range(120..90000u32));
    let hashes = pool(20, || hex(rng, 8));
    let agents = [
        "\"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36\"",
        "\"Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0\"",
        "\"curl/8.5.0\"",
        "\"python-requests/2.32.3\"",
        "\"Go-http-client/2.0\"",
    ];
    let mut out = String::new();
    for _ in 0..lines {
        let ip = pick(rng, &ips);
        let stamp = pick(rng, &stamps);
        let (path, status) = match rng.gen_range(0..8) {
            0 | 1 => (format!("GET /api/v1/items/{} HTTP/1.1", pick(rng, &ids)), 200),
            2 => (format!("POST /api/v1/orders HTTP/1.1"), 201),
            3 => (format!("GET /static/app.{}.js HTTP/1.1", pick(rng, &hashes)), 200),
            4 => ("GET /login HTTP/1.1".to_string(), 200),
            5 => (format!("GET /api/v1/items/{} HTTP/1.1", pick(rng, &ids)), 404),
            6 => ("GET /healthz HTTP/1.1".to_string(), 200),
            _ => ("POST /api/v1/session HTTP/1.1".to_string(), 401),
        };
        out.push_str(&format!("{ip} - - {stamp} \"{path}\" {status} {} \"-\" {}\n", pick(rng, &sizes), pick(rng, &agents)));
    }
    out
}

fn env_file(rng: &mut Rng8, blocks: usize) -> String {
    let pws = pool(40, || word_pw(rng, 20));
    let b43 = pool(40, || b64(rng, 43));
    let b32 = pool(40, || b64(rng, 32));
    let h64 = pool(40, || hex(rng, 64));
    let h40 = pool(40, || hex(rng, 40));
    let h32 = pool(40, || hex(rng, 32));
    let nums = pool(20, || rng.gen_range(100000..999999u32));
    let mut out = String::new();
    for _ in 0..blocks {
        out.push_str("# Application settings\nAPP_ENV=production\nPORT=8080\nLOG_LEVEL=info\n");
        out.push_str(&format!("DATABASE_URL=postgres://app:{}@db.internal:5432/orders\n", pick(rng, &pws)));
        out.push_str(&format!("REDIS_URL=redis://:{}@cache.internal:6379/0\n", pick(rng, &pws)));
        out.push_str(&format!("SESSION_SECRET={}\n", pick(rng, &b43)));
        out.push_str(&format!("JWT_SECRET={}\n", pick(rng, &h64)));
        out.push_str(&format!("API_TOKEN={}\n", pick(rng, &h40)));
        out.push_str(&format!("WEBHOOK_SIGNING_SECRET={}\n", pick(rng, &b32)));
        out.push_str(&format!("SMTP_PASSWORD={}\n", pick(rng, &pws)));
        out.push_str(&format!("ENCRYPTION_KEY={}=\n", pick(rng, &b43)));
        out.push_str(&format!("SENTRY_DSN=https://{}@o{}.ingest.example.com/{}\n\n", pick(rng, &h32), pick(rng, &nums), pick(rng, &nums)));
    }
    out
}
