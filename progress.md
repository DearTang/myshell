# 杩涘害鏃ュ織

## 浼氳瘽锛?026-06-12 ~ 2026-06-13

### 闃舵 1锛氶渶姹傚垎鏋愪笌鎶€鏈€夊瀷
- **鐘舵€侊細** complete

### 闃舵 2锛氱幆澧冩惌寤轰笌椤圭洰鍒濆鍖?
- **鐘舵€侊細** in_progress
- Rust 宸ュ叿閾惧畨瑁呬腑锛坮ustup stable 1.96.0, 涓嬭浇7涓粍浠朵腑锛?
- npm 渚濊禆宸插畨瑁呭畬鎴?
- TypeScript 缂栬瘧妫€鏌ラ€氳繃锛堟棤閿欒锛?

### 闃舵 3锛氭牳蹇冨悗绔疄鐜?(Rust/Tauri)
- **鐘舵€侊細** complete锛堜唬鐮佸凡缂栧啓锛?
- 宸插垱寤?淇敼鐨勬枃浠讹細
  - `src-tauri/Cargo.toml` 鈥?Rust 渚濊禆閰嶇疆
  - `src-tauri/build.rs` 鈥?Tauri 鏋勫缓鑴氭湰
  - `src-tauri/tauri.conf.json` 鈥?Tauri 搴旂敤閰嶇疆
  - `src-tauri/src/main.rs` 鈥?涓诲叆鍙?+ Tauri commands
  - `src-tauri/src/db.rs` 鈥?SQLite 杩炴帴閰嶇疆瀛樺偍
  - `src-tauri/src/ssh.rs` 鈥?SSH 杩炴帴绠＄悊 (russh)
  - `src-tauri/src/sftp.rs` 鈥?SFTP 鏂囦欢鎿嶄綔 (russh-sftp)

### 闃舵 3.1锛氫慨澶?SSH 杈撳嚭鍥炴樉锛?026-06-13锛?
- **鐘舵€侊細** complete锛堝墠绔唬鐮佸凡楠岃瘉锛屽悗绔唬鐮佸凡缂栧啓浣嗘湭缂栬瘧锛?
- **闂锛?* 鍘?ssh.rs 鍒涘缓浜?output_tx 閫氶亾浣嗕粠鏈惎鍔ㄨ鍙栦换鍔★紝瀵艰嚧鏈嶅姟鍣ㄨ緭鍑烘棤娉曞洖鍒?xterm.js锛堝彧鑳界湅鍒?PTY echo 鐨勬湰鍦拌緭鍏ワ級
- **淇鏂规锛?* 閲嶆瀯 SshSession 缁撴瀯 鈥?灏?Channel 鎵€鏈夋潈绉讳氦缁欑嫭绔嬬殑 tokio reader 浠诲姟
  - 鏂板 `SessionCommand` 鏋氫妇锛圛nput/Resize/Disconnect锛夐€氳繃 mpsc 閫氶亾浼犻€掔粰 reader
  - `channel_reader` 浠诲姟鐢?`tokio::select! { biased; ... }` 澶氳矾澶嶇敤锛?
    - `command_rx.recv()` 鈥?鍓嶇鍛戒护锛堜紭鍏堢骇鏈€楂橈紝杈撳叆寤惰繜浣庯級
    - `channel.wait()` 鈥?鏈嶅姟鍣ㄦ暟鎹紝缂撳啿鍒?Vec<u8>
    - `flush_interval.tick()` 鈥?16ms 瀹氭椂鍒锋柊锛屽悎骞剁獊鍙戣緭鍑虹紦瑙?tauri#13234
  - 16KB 闃堝€煎湪绾垮埛鏂帮紙閬垮厤楂樿緭鍑烘椂瀹氭椂鍣ㄥ垎鏀ゥ楗匡級
  - 閫氳繃 `app.emit("ssh_output"|"ssh_closed"|"ssh_exit", payload)` 鎺ㄩ€佸埌鍓嶇
- **淇敼鐨勬枃浠讹細**
  - `src-tauri/src/ssh.rs` 鈥?澶ф敼锛氭柊澧?SessionCommand銆乧hannel_reader 浠诲姟銆乫lush_buffer 杈呭姪鍑芥暟锛泂end_input/resize_terminal/disconnect 鏀逛负鍙戝懡浠?
  - `src-tauri/src/main.rs` 鈥?`ssh_sessions` 鏀逛负 `Arc<Mutex<...>>`锛泂sh_connect 娉ㄥ叆 `AppHandle` 鍙傛暟锛堜慨澶嶄簡鍘?`state.app_handle()` 涓嶅瓨鍦ㄧ殑 bug锛夛紱鏂板 `SshOutputPayload` 搴忓垪鍖栫粨鏋?
  - `src/api.ts` 鈥?鏂板 `onSshOutput`/`onSshClosed`/`onSshExit` 鐩戝惉鍣紙鎸?sessionId 杩囨护锛?
  - `src/components/TerminalPanel.tsx` 鈥?useEffect 鍐呰闃?ssh_output锛堝啓鍏?xterm锛夊拰 ssh_closed锛堢孩鑹?[Connection closed] 鎻愮ず锛夛紱鍗歌浇鏃跺弽璁㈤槄
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛坋xit code 0锛?
- **閬楃暀椋庨櫓锛?*
  - `channel.wait()` 鍦?select! 涓殑鍙栨秷瀹夊叏鎬?鈥?鑻ュ疄娴嬩涪瀛楄妭锛屾敼鐢?`make_reader()` + `tokio::io::split()`
  - `Vec<u8>` JSON 搴忓垪鍖栦负鏁板瓧鏁扮粍锛垀4脳 鑶ㄨ儉锛夆€?v0.1 鍙帴鍙楋紝濡?profiling 鏄剧ず鐡堕鍐嶅垏 base64
  - tauri#13234 鍦ㄥぇ閲忚緭鍑烘椂浠嶅彲鑳藉崱椤?鈥?16ms 鍚堝苟宸茬紦瑙ｏ紝蹇呰鏃跺澶ч棿闅?

### 闃舵 3.2锛氬畨鍏ㄥ鏌ヤ笌楂樺嵄淇锛?026-06-13锛?
- **鐘舵€侊細** complete锛堝墠绔?tsc 閫氳繃锛屽悗绔唬鐮佸凡鍐欏緟 cargo 缂栬瘧锛?
- **瀹℃煡鍙戠幇锛?* 骞惰鎵ц瀹夊叏瀹℃煡 + 璁捐瀹℃煡涓や釜 agent
  - 璁捐瀹℃煡纭锛歁utex 閫夋嫨姝ｇ‘銆乻elect! 鍙栨秷瀹夊叏銆乺eader 鑷Щ闄ゆ棤 TOCTOU銆丼FTP 涓嶄笌 bash 閫氶亾鍐茬獊
  - 瀹夊叏瀹℃煡鍙戠幇 2 涓?HIGH + 3 涓?MEDIUM + 5 涓?LOW
- **宸蹭慨澶嶏紙HIGH/MEDIUM 绔嬪嵆淇級锛?*
  - **HIGH #1锛氳法绐楀彛缁堢娉勬紡** 鈥?`app.emit` 骞挎挱鍒版墍鏈?webview銆傛敼涓?`window.emit` 閫氳繃 `WebviewWindow` 浠呭彂缁欐簮绐楀彛銆備慨鏀?`ssh.rs::connect/channel_reader/flush_buffer` 绛惧悕 `AppHandle 鈫?WebviewWindow`锛沗main.rs::ssh_connect` 鍚屾
  - **HIGH #2锛氱紦鍐插尯鏃犱笂闄愬鑷村墠绔?OOM** 鈥?鏂板 `MAX_BUFFER_SIZE = 256KB` 甯搁噺涓?`append_capped` 杈呭姪鍑芥暟銆傝秴闄愭椂鍒锋柊宸叉湁鏁版嵁 + 鍐欏叆 `[output truncated]` 鏍囪 + 涓㈠純鍓╀綑
  - **MEDIUM #4锛氱鍙ｉ獙璇?* 鈥?`ConnectionDialog.tsx` 鐢?`parseInt(port, 10)` 鏄惧紡鏍￠獙 `1-65535`锛屾浛浠ｉ潤榛?`|| 22` 鍥為€€
- **閬楃暀鏈慨锛?*
  - MEDIUM #3锛歛pp 閫€鍑轰笉娓呯悊 active sessions锛堣祫婧愭硠婕忥級鈥?闇€鍔?`RunEvent::Exit` handler
  - MEDIUM #5锛歚load_secret_key` 璺緞鏈鑼冨寲锛堟枃浠跺瓨鍦ㄦ€?oracle锛?
  - LOW锛歋FTP 姣忔寮€鏂板瓙閫氶亾锛堝欢杩燂紝缂撳瓨 `SftpSession` 鍙紭鍖栵級銆乴istener 寰换鍔￠棿闅欐硠婕忋€丼QLite 鏂囦欢鏉冮檺銆丼FTP rename 鍏佽 `../`銆乺eader 浠诲姟 JoinHandle 鏈窡韪?

### 闃舵 4锛氬墠绔?UI 瀹炵幇
- **鐘舵€侊細** complete锛堜唬鐮佸凡缂栧啓锛?
- 宸插垱寤?淇敼鐨勬枃浠讹細
  - `package.json` 鈥?鍓嶇渚濊禆
  - `vite.config.ts` 鈥?Vite 鏋勫缓閰嶇疆
  - `tsconfig.json` 鈥?TypeScript 閰嶇疆
  - `index.html` 鈥?HTML 鍏ュ彛
  - `src/main.tsx` 鈥?React 鍏ュ彛
  - `src/vite-env.d.ts` 鈥?Vite 绫诲瀷澹版槑
  - `src/styles/global.css` 鈥?鍏ㄥ眬鏍峰紡锛圕atppuccin Mocha 涓婚锛?
  - `src/api.ts` 鈥?Tauri IPC 鎺ュ彛灏佽
  - `src/App.tsx` 鈥?涓诲簲鐢紙甯冨眬 + 澶氭爣绛撅級
  - `src/components/Sidebar.tsx` 鈥?杩炴帴绠＄悊渚ц竟鏍?
  - `src/components/TabBar.tsx` 鈥?鏍囩鏍?+ SFTP 鍒囨崲
  - `src/components/TerminalPanel.tsx` 鈥?xterm.js 缁堢闈㈡澘
  - `src/components/SftpPanel.tsx` 鈥?SFTP 鏂囦欢娴忚鍣?
  - `src/components/ConnectionDialog.tsx` 鈥?杩炴帴閰嶇疆瀵硅瘽妗?

### 闃舵 5锛氶泦鎴愭祴璇曚笌浜や粯
- **鐘舵€侊細** in_progress
- **閲岀▼纰戯紙2026-06-13锛夛細cargo build 棣栨鎴愬姛**
- **楠岃瘉姝ラ锛堟寜搴忔墽琛岋級锛?*
  1. `cargo build`锛堝湪 `src-tauri/`锛夆€?**PASS**锛?1.10s锛岄浂璀﹀憡闆堕敊璇級
  2. `cargo tauri dev`锛堝湪 `src-tauri/`锛夆€?寰呮墽琛?
  3. 杩炴帴鐪熷疄 SSH 鏈嶅姟鍣紙濡?docker `linuxserver/openssh-server` 绔彛 2222锛夐獙璇侊細
     - 鏈嶅姟鍣ㄦ彁绀虹鍑虹幇鍦ㄧ粓绔紙杈撳嚭鍥炶矾宸查€氾級
     - 杈撳叆 `ls`銆乣echo hello`銆乣pwd` 鈥?杈撳嚭鍙
     - 杈撳叆 `ls /nonexistent` 鈥?stderr 鍙
     - 璋冩暣绐楀彛灏哄 鈥?`stty size` 鏄剧ず姝ｇ‘琛屽垪鏁?
     - 鍏抽棴鏍囩 鈥?`ssh_disconnect` 瑙﹀彂锛屾棤娉勬紡
     - 鏈嶅姟鍣ㄤ晶 kill 鐢ㄦ埛 shell 鈥?鏍囩鏄剧ず绾㈣壊 [Connection closed]

### 闃舵 6锛歷0.1 鍗犱綅椤规竻鐞嗭紙2026-06-14锛?
- **鐘舵€侊細** complete锛圧ust 缂栬瘧 PASS锛孴S 绫诲瀷妫€鏌?PASS锛涚鍒扮楠岃瘉寰呰窇锛?
- **CLAUDE.md 鍒楀嚭鐨勪笁澶勫崰浣嶏細**
  1. `check_server_key` 鍏ㄧ洏鎺ュ彈 鈫?DB 姣斿
  2. 瀵嗙爜鏄庢枃瀛?SQLite 鈫?OS keyring
  3. app 鍏抽棴涓?disconnect 浼氳瘽 鈫?閫€鍑烘椂 drain
- **淇敼鏄庣粏锛?*
  - `src-tauri/Cargo.toml` 鈥?鍔?`keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }`
  - `src-tauri/src/main.rs` 鈥?`AppState.db` 鏀?`Arc<Mutex<rusqlite::Connection>>`锛坔andler 鎸佷箙寮曠敤闇€瑕?Arc锛夛紱`save_connection` 鎶婂瘑鐮佷粠 config 鍙栧嚭鍐欏叆 keyring 鍚?DB 瀛?NULL锛沗delete_connection` 鍏?best-effort 鍒?keyring锛涘惎鍔ㄨ皟 `migrate_plaintext_passwords`锛沗.run()` 鏀?`.build()` + `ExitRequested` 鐩戝惉 鈫?`drain_ssh_sessions` 鍙?Disconnect + sleep 500ms
  - `src-tauri/src/db.rs` 鈥?`CREATE TABLE known_hosts`锛沗get_known_host`/`set_known_host`锛沗migrate_plaintext_passwords`锛堜竴娆℃€ф妸 NOT NULL 鐨勬槑鏂囨惉鍒?keyring 骞?NULL 鍖栵級
  - `src-tauri/src/secrets.rs` 鈥?鏂版ā鍧楋紝keyring::Entry 鍖呰锛坰et/get/delete锛?
  - `src-tauri/src/ssh.rs` 鈥?`SshClient { db: Arc<Mutex<Connection>>, host }`锛沗check_server_key` 璧?DB 姣斿锛氬尮閰嶆帴鍙椼€佷笉鍖归厤鎷掔粷銆侀娆″啓鍏ワ紱`connect()` 鏋勯€?handler 鏃舵敞鍏?db Arc clone + host锛沺assword 璁よ瘉濡?config.password 绌哄氨浠?keyring 璇伙紱鍒犻櫎姝讳唬鐮?`windows_match`
  - `src/components/ConnectionDialog.tsx` 鈥?缂栬緫鏃剁┖瀵嗙爜瀛楁浼?`undefined`锛堜繚鐣?keyring 鏃у€硷紝涓嶅啀璇垹锛夛紱鏂板缓 + password 璁よ瘉 + 瀵嗙爜绌?鈫?鎶ラ敊"璇峰～鍐欏瘑鐮?
- **閬楃暀椋庨櫓锛?*
  - Linux keyring 璧?Secret Service锛堥渶瑕?D-Bus + gnome-keyring/kwallet 杩愯锛夛紱Windows/macOS 鐢ㄥ師鐢燂紙Credential Manager / Keychain锛夛紝鏃犱緷璧?
  - known_hosts 鏄?host 缁村害锛堜笉鍒嗙鍙?绠楁硶锛夆€?鍚?host 涓嶅悓绔彛鐨勬伓鎰忔湇鍔＄浼氳Е鍙?mismatch"璇嫆锛泇0.1 鎺ュ彈
  - keyring 鍐欏け璐ユ椂 save_connection 鏁翠綋澶辫触锛屼絾 DB 宸茬粡鍐欎簡锛圛NSERT OR REPLACE 鏄竴涓簨鍔″唴鐨勶紝鏈皟 set_password 涓嶄細鍏堝姩 DB锛夆€?瀹為檯椤哄簭鏄?set_password 鈫?db::save_connection锛宬eyring 澶辫触鏃?DB 鏈姩锛孫K


- **鐘舵€侊細** complete
- **璧峰洜锛?* rustup 瑁呭ソ鍚庨娆?`cargo build`锛岃繛缁懡涓?7 绫婚樆濉為棶棰?
- **淇鏄庣粏锛?*
  1. **`russh-keys = "0.50"` 鏃?stable 鐗堟湰**锛?.50 鍙湁 beta锛?.50.0-beta.7锛夈€備粠 `Cargo.toml` 绉婚櫎鐩存帴渚濊禆锛屾敼鐢?`russh::keys` 鐨?re-export锛坄PublicKey`銆乣load_secret_key`銆乣PrivateKeyWithHashAlg`锛?
  2. **Windows schannel CRYPT_E_REVOCATION_OFFLINE (0x80092013)**锛氬悐閿€鏈嶅姟鍣ㄦ棤娉曡闂€傚湪 `.cargo/config.toml` 鍔?`[http] check-revoke = false`
  3. **`russh-sftp 1.2.1` 涓?`bytes 1.10` 鍐茬獊**锛歜ytes 1.10 鍦?`Buf` trait 涓婂姞浜?`try_get_*` 鏂规硶锛屼笌 russh-sftp 鑷繁鐨?`TryBuf::try_get_u32` 鍐茬獊锛坱okio-rs/bytes#767锛夈€傚崌绾у埌 `russh-sftp = "2"`
  4. **`tauri.conf.json` 椤跺眰 `title` 瀛楁搴熷純**锛氭柊鐗?tauri-build 2.6.2 瑕佹眰 `title` 鍦?`windows[]` 鍐呫€傚垹闄ら《灞?`app.title`锛岄『鎵嬫妸閿欒鐨?`$schema` 浠?nicegui 鏀规垚 Tauri 瀹樻柟 `https://schema.tauri.app/config/2`
  5. **缂?`icons/icon.ico`**锛氱敤 PowerShell System.Drawing 鐢熸垚 32脳32 鍗犱綅 ICO锛堣摑鍦?+ Catppuccin 娣辫壊鑳屾櫙锛?
  6. **russh 0.50.4 API 鍙樻洿**锛?
     - `authenticate_publickey` 绗簩鍙傛暟浠?`Arc<PrivateKey>` 鈫?`PrivateKeyWithHashAlg::new(Arc<PrivateKey>, None)`
     - `AuthResult` 鏀逛负 enum锛屾彁渚?`.success()` 鏂规硶鏇夸唬 `!` 鎿嶄綔
     - `Handle<H>` 涓嶅啀瀹炵幇 `Clone` 鈫?鎶?`SshSession.handle` 鏀规垚 `Arc<Handle<SshClient>>`
     - `CryptoVec` 鏃?`as_slice()` 鈫?鐢?`&data[..]`锛圖eref 鍒?`[u8]`锛?
     - `SshClient` 蹇呴』鏄?`pub`锛堝惁鍒?`pub handle: Arc<Handle<SshClient>>` 鏆撮湶绉佹湁绫诲瀷锛?
  7. **russh-sftp 2.x API 鍙樻洿**锛?
     - `mkdir` 鈫?`create_dir`
     - `rm_dir` 鈫?`remove_dir`
     - `FilePermissions` 涓嶅啀瀹炵幇 `Debug`锛屼絾鏈?`Display` 鈫?`format!("{}", ...)` 鏇夸唬 `{:?}`
  8. **`MutexGuard` 璺?await 瀵艰嚧 Future 涓?Send**锛歵auri command 鐨勮繑鍥?Future 蹇呴』 Send銆俙get_sftp_session` 鐢ㄥ潡浣滅敤鍩熷寘浣忛攣锛岀‘淇?guard 鍦?await 鍓嶉噴鏀?
  9. **`src/main.rs` 缂?`fn main()`**锛氬師浠ｇ爜鍙畾涔変簡 `pub fn run()`銆傝ˉ `fn main() { run() }`锛屼繚鐣?`#[cfg_attr(mobile, tauri::mobile_entry_point)]` 鍏煎绉诲姩绔?
- **淇敼鐨勬枃浠讹細**
  - `src-tauri/Cargo.toml` 鈥?绉婚櫎 `russh-keys`锛宍russh-sftp` 鍗?2
  - `src-tauri/.cargo/config.toml` 鈥?鍔?`check-revoke = false`
  - `src-tauri/tauri.conf.json` 鈥?鍒?`app.title`锛屼慨 `$schema`
  - `src-tauri/icons/icon.ico` 鈥?鏂板缓鍗犱綅鍥炬爣
  - `src-tauri/src/ssh.rs` 鈥?handle 鏀?Arc 鍖呰锛涜璇?API 鍗囩骇锛汣ryptoVec 鍒囩墖璇硶锛汼shClient 鍔?pub
  - `src-tauri/src/sftp.rs` 鈥?Handle clone 鏀?Arc::clone锛沵kdir/rm_dir 鏀瑰悕锛沺ermissions Display锛汳utexGuard 鍧椾綔鐢ㄥ煙
  - `src-tauri/src/main.rs` 鈥?琛?`fn main()`

## 娴嬭瘯缁撴灉
| 娴嬭瘯 | 杈撳叆 | 棰勬湡缁撴灉 | 瀹為檯缁撴灉 | 鐘舵€?|
|------|------|---------|---------|------|
| TypeScript 缂栬瘧 | npx tsc --noEmit | 鏃犻敊璇?| 鏃犻敊璇?| PASS |
| npm install | package.json | 瀹夎鎴愬姛 | 鎴愬姛 | PASS |
| Rust 瀹夎 | rustup default stable | 瀹夎鎴愬姛 | 鎴愬姛 | PASS |
| Rust 缂栬瘧 | cargo build | 鏃犻敊璇?| 11.10s 瀹屾垚 | PASS |

## 閿欒鏃ュ織
| 鏃堕棿鎴?| 閿欒 | 灏濊瘯娆℃暟 | 瑙ｅ喅鏂规 |
|--------|------|---------|---------|
| 2026-06-12 | rustup default 鏈缃?| 1 | 鎵ц rustup default stable |
| 2026-06-12 | rustup 涓嬭浇鑰楁椂杩囬暱 | 1 | 鍚庡彴缁х画锛岀瓑寰呭畬鎴?|

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 3.3锛堢紪璇戜慨澶嶏級瀹屾垚锛宍cargo build` PASS锛?1.10s 闆惰鍛婏級锛涜繘鍏ラ樁娈?5 绔埌绔獙璇?|
| 鎴戣鍘诲摢閲岋紵 | 鍚姩 `cargo tauri dev`锛岃繛鐪熷疄 SSH 鏈嶅姟鍣ㄨ窇瀹?6 椤归獙璇佹竻鍗?|
| 鐩爣鏄粈涔堬紵 | 鏋勫缓鍙繍琛岀殑 MyShell SSH/SFTP 瀹㈡埛绔?|
| 鎴戝鍒颁簡浠€涔堬紵 | russh 0.50 涓?0.49 API 宸紓宸ㄥぇ锛圚andle 涓?Clone銆丄uthResult enum銆丳rivateKeyWithHashAlg锛夛紱russh-sftp 1.x 涓?bytes 1.10 涓嶅吋瀹癸紱async fn 涓?std::sync::MutexGuard 鍗充究 drop 涔熶細璁?Future 涓?Send锛屽繀椤诲潡浣滅敤鍩燂紱tauri-build 2.6 鍒犻櫎浜?`app.title` 瀛楁 |
| 鎴戝仛浜嗕粈涔堬紵 | 3.3锛氳繛缁慨 9 绫荤紪璇戦樆濉烇紙russh-keys 缂哄け銆丼SL 鍚婇攢銆乺ussh-sftp 鍗囩骇銆乼auri.conf 瀛楁銆乮con 缂哄け銆? 澶?API 鍗囩骇銆丮utexGuard Send銆乵ain 鍑芥暟缂哄け锛?|

---
*姣忎釜闃舵瀹屾垚鍚庢垨閬囧埌閿欒鏃舵洿鏂版鏂囦欢*

## 浼氳瘽锛?026-06-14 鈥?5 澶ч渶姹傚寮?

### 闃舵 7锛氶渶姹傛墿灞曪紙A-F 鍏ㄥ锛?
- **鐘舵€侊細** Phase A-E complete / Phase F 寰呰繍琛屾椂楠岃瘉
- **闇€姹傛竻鍗曪細**
  1. Stage 4 鎬ц兘涓庡仴澹€э紙5GB 澶ф枃浠躲€乤bort 5s 鍏滃簳銆?0 杩炲彂锛?
  2. 澶氱骇鏂囦欢澶癸紙鏍戝舰/榛樿鏀惰捣锛?
  3. SSH/SFTP/FTP 缁熶竴绠＄悊
  4. SSH 鏈嶅姟鍣ㄤ俊鎭晶鏍忥紙OS/CPU/鍐呭瓨/纾佺洏 5s 鍒锋柊锛?
  5. UI 缇庡寲锛堢粓绔潃鑹?+ 瀵硅瘽妗嗗垎缁勶級

### 闃舵 7.A锛欴B schema + 绫诲瀷鎵╁睍
- **鐘舵€侊細** complete
- `db.rs` 鈥?`init_db` 鍔?`conn_type/group_path/ftp_tls/ftp_passive` 鍒?+ `folders` 琛紱`column_exists` PRAGMA 鎺㈡祴锛沗migrate_legacy_schema` 鎸夐『搴忥細`group_name` 鈫?`group_path` 杩佺Щ 鈫?drop column 鈫?drop legacy `password` 鍒楋紱`list/save/delete/rename_folder` + `folder_has_children`锛沗rename_folder` 鐢?`LIKE 'old/%'` 妯″紡鎵归噺鏇存柊瀛愰」璺緞
- `main.rs` 鈥?`ConnectionConfig` 鍔?4 涓瓧娈碉紙甯?`#[serde(default)]` 鍏煎鏃?JSON锛夛紱`AppState.ftp_sessions: Arc<Mutex<HashMap<UUID, FtpSession>>>`

### 闃舵 7.B锛歎I 缇庡寲
- **鐘舵€侊細** complete
- `TerminalPanel.tsx` 鈥?PTY 寤虹珛鍚庢敞鍏?`SHELL_INIT_SEQ`锛坄FORCE_COLOR=1` + `alias ls='ls --color=auto'` + `source ~/.bashrc 2>/dev/null` + `clear`锛夛紝sh/dash/fish 鍏煎锛坄2>/dev/null` 闈欓粯锛夛紱`abortTimeoutRef` 5s setTimeout 璋?`bridgeRef.reset()` 鍏滃簳
- `ConnectionDialog.tsx` 鈥?`TypeSelector`锛圫SH/SFTP/FTP 鎸夐挳 + accent 楂樹寒锛夛紱`FieldGroup` 鍗＄墖鍒嗙粍锛堝熀鏈?璁よ瘉/FTP/鍒嗙粍锛夛紱`nameTouchedRef` 鑷姩鍚屾 host 鈫?name 鐩村埌鐢ㄦ埛鏀?name锛沠ocus 鎬?`boxShadow: 0 0 0 2px rgba(137,180,250,0.18)`锛涘瘑鐮佸瓧娈?warning 鑹茶竟妗嗐€佸瘑閽ヨ矾寰?accent 鑹?

### 闃舵 7.C锛氬绾ф枃浠跺す
- **鐘舵€侊細** complete
- `Sidebar.tsx` 閲嶅啓涓烘爲褰?鈥?`buildTree(conns, folders)` 閫掑綊鏋勯€?`FolderNode { depth, children, conns }`锛沗paddingLeft: 14 + depth * 12`锛沗Set<string>` 灞曞紑鎬佸垵濮嬬┖锛堥粯璁ゅ叏鏀惰捣锛夛紱鍙抽敭鑿滃崟锛氱┖鐧藉鏂板缓杩炴帴/鏂囦欢澶广€佹枃浠跺す椤瑰瓙寤?閲嶅懡鍚?鍒犻櫎锛沗CONN_ICONS: { ssh: "馃枼", sftp: "馃搧", ftp: "馃摛" }`
- `normalize_folder_path` 鍦?Rust 绔鑼冨寲锛堝幓閲?`/`銆佽ˉ鍓嶅 `/`锛夛紱`rename_folder` 鎷掔粷寰幆锛坄new.starts_with(old + "/")`锛夛紱`delete_folder` 鏍￠獙 `folder_has_children`

### 闃舵 7.D锛歋SH 鏈嶅姟鍣ㄤ俊鎭?
- **鐘舵€侊細** complete
- `ssh.rs::exec_once` 鈥?鐭懡浠ゆ墽琛屽姪鎵嬶紙clone `Arc<Handle>`銆佸紑 channel銆乣channel.exec(true, cmd)`銆佸惊鐜?`channel.wait()` 绱Н `Data`/`ExtendedData`锛?
- `main.rs` 鈥?`SERVER_INFO_SCRIPT` 鍗曟 exec 鎷垮叏锛圤S/K/C/M/D/S1/sleep/S2锛夛紝`tokio::time::timeout(8s)` 瓒呮椂杩斿洖 `stale: true`锛沗parse_server_info` 鎸?`=TAG=` 鍒囩墖锛沗cpu_busy_pct` 浠庝袱娆?/proc/stat 绠楀樊鍊?
- `ServerInfoPanel.tsx`锛?40px 渚ф爮锛夆€?`MetricCard` + `UsageBar`锛?85% error / >60% warning / else success锛夛紱5s setInterval锛沗active=false` 鏃惰烦杩囧埛鏂帮紙鍒?tab 鏆傚仠锛?

### 闃舵 7.E锛欶TP 鏀寔锛堟渶澶ф敼鍔級
- **鐘舵€侊細** complete
- **suppaftp v8 API 鎺㈢储韪╁潙锛?*
  - `types::File` 涓嶅瓨鍦?鈫?`list::File`
  - `AsyncRustlsConnector` 闇€瑕佸閮?rustls/webpki-roots 鐩存帴渚濊禆 鈫?鏆傜紦 TLS锛堣繑鍥為敊璇紩瀵奸€?`ftp_tls=none`锛?
  - `passive()` 鏂规硶涓嶅瓨鍦?鈫?v8 榛樿灏辨槸 PASV锛涘垏 active 鐢?`stream.active_mode(timeout)`
  - `mlsd/list` 杩斿洖 `Vec<String>` 涓嶆槸 `Vec<File>` 鈫?蹇呴』鐢?`ListParser::parse_mlsd().or_else(parse_posix)` 閫愯瑙ｆ瀽
- `ftp.rs` 鈥?`FtpSession { stream: AsyncFtpStream }`锛沜onnect/list/mkdir/remove/rename/disconnect锛沗format_pex` POSIX 鏉冮檺涓诧紱`format_time`+`days_to_ymd` 鏃?chrono 杞涓烘棩鏈?
- `main.rs` 鈥?6 涓柊鍛戒护锛汧TP 鍊熻繕鏈?`take_ftp_session`/`return_ftp_session`锛坄AsyncFtpStream` 涓?Clone锛屽繀椤?take 鈫?鐢?鈫?return锛夛紱`drain_all_sessions` 鍚屾 clear FTP map锛坉rop 鍗冲叧 socket锛?
- `api.ts` 鈥?`ftpConnect/ListDir/Mkdir/Remove/Rename/Disconnect` 鍖呰
- `App.tsx::handleConnect` 鈥?鎸?`conn_type` 鍒嗘祦锛歠tp 鈫?鐙珛 FTP tab锛泂ftp 鈫?ssh tab+sftp type锛泂sh 鈫?terminal锛沗handleCloseTab` 鎸?connType 璋冨搴?disconnect
- `SftpPanel.tsx` 鈥?`source: "ssh" | "ftp"` props + `fullHeight`锛涙寜 source 鍒嗗彂锛汧TP 鍒濆璺緞鐢?`/`锛堜笉鏀寔 `~`锛?
- `TabBar.tsx` 鈥?tab 鍥炬爣鎸?connType 鍒囨崲

### 闃舵 7.F锛歋tage 4 楠岃瘉锛堣繍琛屾椂锛?
- **鐘舵€侊細** pending锛堝緟鐢ㄦ埛鍙備笌锛?
- **宸插氨缁唬鐮佷晶锛?* `MAX_BUFFER_SIZE=256KB` 闃插墠绔?OOM锛沍MODEM chunk 8KB锛?s abort setTimeout 鍏滃簳宸插姞
- **寰呰窇娴嬭瘯锛?*
  1. `dd if=/dev/urandom of=big.bin bs=1M count=5120` 鈫?rz + sz 鈫?浠诲姟绠＄悊鍣ㄨ瀵?myshell.exe 宄板€?<100MB
  2. 鍗?tab 杩炵画 10 娆?`sz fileN` 鈫?鐩戞帶 zmodem_files Mutex 涓嶉樆濉?
  3. ZMODEM abort 鈫?5s 鍚庡己鍒?reset 楠岃瘉

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| Phase A-E 鍏ㄩ儴瀹屾垚锛沜argo build + tsc --noEmit 鍙岀豢 |
| 鎴戣鍘诲摢閲岋紵 | Phase F 杩愯鏃堕獙璇侊紱涔嬪悗鍚姩 `cargo tauri dev` 璁╃敤鎴疯繛鐪熷疄鏈嶅姟鍣ㄧ鍒扮娴嬭瘯 |
| 鐩爣鏄粈涔堬紵 | 5 澶ч渶姹傚畬鏁村彲鐢?|
| 鎴戝鍒颁簡浠€涔堬紵 | suppaftp v8 API 澶ф敼锛堟棤 `passive()`/`types::File`锛宮lsd 杩斿洖 String锛夛紱FTP session 鍥?`AsyncFtpStream` 涓?Clone 蹇呴』鐢?take/return 鍊熻繕鏈紱russh 0.50 Handle 鏄?Arc 鍙寮€ channel 鈥?exec channel 涓?PTY channel 鍏卞瓨锛圫erverInfo panel 涓庣粓绔悓鏃跺伐浣滐級 |
| 鎴戝仛浜嗕粈涔堬紵 | Phase A锛欴B schema锛汸hase B锛歎I 缇庡寲锛汸hase C锛氭爲褰㈡枃浠跺す锛汸hase D锛氭湇鍔″櫒淇℃伅渚ф爮锛汸hase E锛欶TP 鍏ㄦ爤 |

## 浼氳瘽锛?026-06-15 鈥?鍏ㄥ眬 + 鏈嶅姟鍣ㄤ笓灞炲揩鎹峰懡浠?

### 闃舵 8锛氬揩鎹峰懡浠ゅ姛鑳斤紙鍏ㄥ眬 + 鏈嶅姟鍣ㄤ笓灞?+ 澶氳椤哄簭鎵ц锛?
- **鐘舵€侊細** complete锛坈argo check + tsc + clippy 鏂颁唬鐮佷笁缁匡紱绔埌绔緟鐢ㄦ埛鎵嬪姩楠岃瘉锛?
- **闇€姹傦細** 璁剧疆涓坊鍔犲叏灞€蹇嵎鍛戒护锛涢拡瀵瑰綋鍓嶆湇鍔″櫒鐨勪笓灞炲揩鎹峰懡浠わ紱澶氳鍛戒护鎸夎椤哄簭鎵ц锛涚粓绔竴閿偣鍑荤洿鎺ユ墽琛?
- **璁捐鍐崇瓥锛堜笌鐢ㄦ埛纭锛夛細**
  - 鏁版嵁妯″瀷锛氬崟琛?`quick_commands`锛宍connection_id` 涓?NULL=鍏ㄥ眬銆侀潪 NULL=鏈嶅姟鍣ㄤ笓灞烇紱鎵ц闈㈡澘涓€鏉?SQL锛坄WHERE connection_id IS NULL OR connection_id = ?`锛夎仈鍚堝彇鍏ㄥ眬+涓撳睘
  - NULL 瀹夊叏锛歞b.rs 鎸?scope 鍒嗘敮鏋勯€?SQL锛圢one鈫抈IS NULL`锛孲ome鈫抈= ?1`锛夛紝瑙勯伩 `connection_id IS ?1` 璺?SQLite 鐗堟湰璇箟椋庨櫓
  - 澶氳鎵ц锛氬墠绔寜 `\r?\n` 鎷嗚鈫抰rim鈫掕烦杩囩┖琛屽拰琛岄 `#` 娉ㄩ噴鈫掔敤 `\r` 鎷兼帴涓€娆℃€?`sshSend`锛圥TY 蹇呴』鐢?`\r` 瑙﹀彂鎵ц锛宍\n` 涓嶈Е鍙戯級锛涘鐢?`sshSend` + 骞挎挱鎵囧嚭
  - 绠＄悊鐣岄潰锛氱嫭绔?`QuickCommandsPanel`锛堜綔鐢ㄥ煙涓嬫媺鍒囨崲 鍏ㄥ眬/浠绘剰鏈嶅姟鍣級锛屽叆鍙ｄ笁澶勶紙渚ц竟鏍?馃З / 缁堢鎵ц闈㈡澘"绠＄悊"閾炬帴 / 璁剧疆闈㈡澘鍐?Section锛?
  - 鐐瑰嚮琛屼负锛氱洿鎺ユ墽琛岋紙涓嶅～杈撳叆妗嗭紝绗﹀悎"蹇嵎"瀹氫綅锛?
  - 涓嶅啓鍏?command_history锛堝琛岃涔変笌鍗曡鍘嗗彶妯″瀷涓嶅尮閰嶏級
  - `delete_connection` 鏀归€犱负浜嬪姟 + 绾ц仈娓呯悊璇ユ湇鍔″櫒鐨勪笓灞炲懡浠わ紙command_history 淇濇寔鐜扮姸涓嶇骇鑱旓紝鍚戝悗鍏煎锛?
- **淇敼鏄庣粏锛?*
  - `src-tauri/src/db.rs` 鈥?`init_db` 鍔?`quick_commands` 琛?+ 2 绱㈠紩锛沗QuickCommandTuple` type alias锛堟秷 clippy complex_type warning锛夛紱6 涓?CRUD锛坄add_quick_command`/`list_quick_commands`/`list_quick_commands_for_connection` 鑱斿悎鏌ヨ/`update_quick_command`/`update_quick_command_order`/`delete_quick_command`锛夛紱`delete_connection` 鏀逛簨鍔＄骇鑱?
  - `src-tauri/src/main.rs` 鈥?`QuickCommandItem`/`QuickCommandExecItem` struct锛? 涓?`#[tauri::command]`锛堝悓姝ワ紝鏄庢枃涓嶈皟 `require_dek`锛夛紱`generate_handler!` 娉ㄥ唽
  - `src/api.ts` 鈥?`QuickCommandItem`/`QuickCommandExecItem` interface + 6 涓?invoke 鍖呰锛堟寜 connectionId 閿帶锛屼笌 command_history 鍚岀害瀹氾級
  - `src/components/QuickCommandsPanel.tsx`锛堟柊寤猴級鈥?缁熶竴绠＄悊闈㈡澘锛氫綔鐢ㄥ煙涓嬫媺鍒囨崲 + CRUD 鍒楄〃 + 鍐呰仈缂栬緫琛ㄥ崟 + 鈫戔啌鎺掑簭锛堜氦鎹㈢浉閭?sortOrder锛?
  - `src/components/CommandBar.tsx` 鈥?`鈱?蹇嵎` 鎸夐挳 + 娴眰闈㈡澘锛堭煂?鍏ㄥ眬 / 馃搶 鏈湇鍔″櫒涓撳睘 鍒嗙粍锛? `handleExecuteQuickCommand`锛堝琛?`\r` 鎷兼帴 + 璺宠繃绌鸿/娉ㄩ噴 + 骞挎挱鎵囧嚭锛? `QuickCommandGroup` 瀛愮粍浠?
  - `src/components/TerminalPanel.tsx` 鈥?閫忎紶 `onOpenQuickCommandsManage` prop
  - `src/App.tsx` 鈥?`showQuickCommands`/`qcInitialConnectionId` state + 闈㈡澘娓叉煋 + 涓夊叆鍙ｆ帴绾匡紙Sidebar/CommandBar 绠＄悊/SettingsPanel锛?
  - `src/components/Sidebar.tsx` 鈥?馃З 鎸夐挳锛堝悓娆?IconBtn锛?
  - `src/components/SettingsPanel.tsx` 鈥?"蹇嵎鍛戒护" Section 鍏ュ彛
- **楠岃瘉锛?*
  - `cargo check` PASS锛?m52s锛岄娆＄紪璇?tauri 鍏ㄥ渚濊禆锛涢浂閿欒闆惰鍛婏級
  - `cargo clippy` 鏂颁唬鐮?0 warning锛堜慨澶?`list_quick_commands` 鐨?"very complex type" 鈫?`QuickCommandTuple` type alias锛夛紱鍓╀綑 5 涓?warning 鍧囦负椤圭洰棰勫瓨锛坰sh.rs:249 / db.rs:243 / backup.rs:187 / main.rs:303 / backup create锛?
  - `npx tsc --noEmit` PASS
- **寰呮墜鍔?E2E锛堥渶鐪熷疄 SSH 鏈嶅姟鍣級锛?*
  1. 绠＄悊闈㈡澘锛氬叏灞€鏂板 `echo global`锛涘綋鍓嶆湇鍔″櫒鏂板澶氳 `cd /tmp`+`# 娉ㄩ噴`+`echo $PWD`
  2. 缁堢 `鈱?蹇嵎`锛氫袱缁勫懡浠ゅ垎缁勬樉绀猴紝鐐瑰嚮鐩存帴鎵ц锛涘琛屾寜椤哄簭銆佺┖琛?娉ㄩ噴璺宠繃
  3. 骞挎挱锛氫袱 tab 骞挎挱锛岀偣蹇嵎鍛戒护鍚屾鎵ц
  4. 绾ц仈锛氬垹闄ゆ湇鍔″櫒锛屼笓灞炲懡浠よ娓呫€佸叏灞€淇濈暀锛涢噸杩炲悗闈㈡澘浠嶈兘鍒楀嚭骞舵墽琛?
- **閬楃暀椋庨櫓/闄愬埗锛?*
  - heredoc锛坄<<EOF`锛変綋鍐?`#` 寮€澶磋/绌鸿浼氳杩囨护璇垹 鈥?闈㈡澘鏂囨宸叉彁绀猴紝鏈潵鍙姞 per-command raw 寮€鍏?
  - 澶氳鎵ц渚濊禆 PTY 琛岀紦鍐蹭繚璇侀『搴忥紱`cd` 绛夌姸鎬佷緷璧栧懡浠ら渶鐢ㄦ埛鍦ㄥ悓涓€蹇嵎鍛戒护鍐呭啓濂?
  - 鍥炬爣閬垮紑 `鈿锛堥噸杩炴寜閽凡鍗犵敤锛孋ommandBar.tsx:186锛夛細鎵ц鎸夐挳鐢?`鈱╜锛岀鐞嗗叆鍙ｇ敤 `馃З`
- **棣栨缂栬瘧韪╁潙锛?* `cargo check` 棣栨鍥犵綉缁滀腑鏂笅杞?`web-sys`锛坮eqwest鈫抰auri 渚濊禆锛夊け璐ワ紙`schannel: server closed abruptly`锛夛紝閲嶈瘯涓€娆″悗鎴愬姛

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 8 蹇嵎鍛戒护鍔熻兘瀹屾垚锛沜argo check + tsc + clippy锛堟柊浠ｇ爜锛変笁缁?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛杩愯 `cargo tauri dev` 杩炵湡瀹炴湇鍔″櫒璺?E2E锛堝琛?`\r` 鎵ц椤哄簭鏄獙璇侀噸鐐癸級 |
| 鐩爣鏄粈涔堬紵 | 鍏ㄥ眬+鏈嶅姟鍣ㄤ笓灞炲揩鎹峰懡浠わ紝澶氳鎸夎椤哄簭鎵ц锛岀粓绔竴閿偣鍑荤洿鎺ヨ繍琛?|
| 鎴戝鍒颁簡浠€涔堬紵 | command_history 鏄揩鎹峰懡浠ょ殑澶╃劧妯℃澘锛堝悓娆惧缓琛?CRUD/IPC/鎵ц閫氶亾锛夛紱PTY 澶氳蹇呴』鐢?`\r` 鎷兼帴鑰岄潪 `\n`锛沗connection_id IS ?1` 鐨?NULL 璇箟璺ㄧ増鏈湁椋庨櫓锛屽簲鎸?scope 鍒嗘敮鏋勯€?SQL锛況usqlite `query_map` 涓ゅ垎鏀嫢鐢ㄤ笉鍚岄棴鍖呭瓧闈㈤噺浼氱被鍨嬩笉涓€鑷达紝闇€鎻愬彇鎴愬嚱鏁版寚閽堬紙fn 绫诲瀷锛夊鐢?|
| 鎴戝仛浜嗕粈涔堬紵 | 闃舵 8锛歞b 寤鸿〃+CRUD+绾ц仈锛沵ain struct+commands+娉ㄥ唽锛沘pi.ts 灏佽锛汣ommandBar 鎵ц闈㈡澘锛決uickCommandsPanel 绠＄悊闈㈡澘锛汚pp/Sidebar/SettingsPanel/TerminalPanel 鎺ョ嚎 |

## 浼氳瘽锛?026-06-15 鈥?鏃ュ織澧炲己锛? 澶╀繚鐣?+ 缁撴瀯鍖栬瘖鏂棩蹇楋級

### 闃舵 9锛氭棩蹇楃郴缁熷姞鍥?
- **鐘舵€侊細** complete锛坈argo clippy 閫氳繃锛屾柊浠ｇ爜 0 warning锛?
- **闇€姹傦細** 澶氬鍔犳棩蹇楁柟渚垮悗缁畾浣嶉棶棰橈紱榛樿鍒犻櫎 7 澶╁墠鐨勬棩蹇?
- **鐜版湁鏈哄埗鍙戠幇锛?* release 妯″紡宸叉湁 `setup_file_logging`锛圵indows CRT `_open_osfhandle` + `_dup2` 鎶?stderr fd 2 閲嶅畾鍚戝埌鎸夊ぉ鍛藉悕鐨勬枃浠?`<config_dir>/myshell/logs/myshell-{day}.log`锛夛紝鎵€鏈?`eprintln!` 宸茶惤鐩橈紱浣嗕繚鐣欐湡鏄?14 澶╋紝涓?`env_logger::init()` 榛樿 error 绾у埆瀵艰嚧 `log::info!/warn!` 琚繃婊ゆ帀
- **鏀瑰姩鏄庣粏锛?*
  - `src-tauri/src/main.rs` `setup_file_logging`
    - 娓呯悊闃堝€?14 澶?鈫?**7 澶?*锛坄60*60*24*14` 鈫?`*7`锛屽惈 doc 娉ㄩ噴锛?
    - 娓呯悊閫昏緫缁熻鍒犻櫎鏁伴噺锛坄pruned: u32` 璁℃暟锛夛紝鍦?dup2 瀹屾垚鍚庣殑 startup banner 杈撳嚭锛坉up2 涔嬪墠鐨勬棩蹇楀啓鍘熷 stderr锛宺elease 浼氫涪澶憋級
    - startup banner 浠?`eprintln!`锛堟棤鏍煎紡 epoch 绉掞級鈫?`log::info!`锛堝甫绾у埆/鏃堕棿鎴?淇濈暀鏈?娓呯悊鏁帮級
  - `src-tauri/src/main.rs` `run()`
    - `env_logger::init()` 鈫?`Builder::from_env(Env::default().default_filter_or("info")).format_timestamp_millis()` 鈥斺€?璁?log 瀹忓湪榛樿绾у埆鐢熸晥锛汻UST_LOG 浠嶅彲瑕嗙洊锛堝 `RUST_LOG=myshell=debug` 鎺掓煡锛?
    - 鍚姩闃舵鏃ュ織锛歞b 鍒濆鍖?info銆乻chema 杩佺Щ缁撴灉锛堝け璐?warn / 鎴愬姛 info锛夈€乥ackup 妫€鏌ュけ璐?warn
  - `src-tauri/src/main.rs` 鍏抽敭璇婃柇璺緞
    - `ssh_connect`锛氳姹傦紙user@host:port + auth + proxy锛夈€佹垚鍔燂紙sid + target锛夈€佸け璐ワ紙error + 鍘熷洜锛?
    - `ssh_disconnect`锛歞isconnect requested
    - `delete_connection`锛歬eyring 鍒犻櫎澶辫触 warn銆佸畬鎴?info锛堢骇鑱旀竻鐞嗘彁绀猴級
    - 蹇嵎鍛戒护 `add_quick_command` / `update_quick_command` / `delete_quick_command`锛歩nfo锛坕d / label / scope锛?
  - `src-tauri/src/ssh.rs` `channel_reader`锛氬惎鍔?started + 閫€鍑?exited锛坕nfo锛夆€斺€?瀹氫綅 SSH 杈撳嚭涓柇/浼氳瘽缁撴潫鐨勫叧閿?
- **淇濈暀鏈敼锛?* 鐜版湁 `eprintln!`锛坉ebug 鏁版嵁娴?`Data N bytes`銆丳TY 姝ラ绛夛級淇濈暀 鈥斺€?release 宸查€氳繃 stderr 閲嶅畾鍚戝啓鍏ユ枃浠讹紝宸ヤ綔姝ｅ父锛岄伩鍏嶅叏閲忔浛鎹㈠紩鍏ラ闄?
- **楠岃瘉锛?* `cargo clippy` PASS锛?.93s 澧為噺缂栬瘧锛? 閿欒锛夛紱鏂颁唬鐮?0 warning锛? 涓?warning 鍧囦负椤圭洰棰勫瓨锛歜ackup sort_by / main.rs manual_strip / main.rs open_options / ssh.rs field assignment / db.rs complex type锛?
- **鏃ュ織鏌ョ湅锛?* release 鐢ㄦ埛鐪?`%APPDATA%/myshell/logs/myshell-{day}.log`锛涘紑鍙?debug 鐪嬫帶鍒跺彴锛涙帓鏌ヨ繛鎺ラ棶棰樻悳 `[ssh]` 鍓嶇紑锛坈onnect requested 鈫?connected/failed 鈫?channel_reader started/exited 瀹屾暣閾捐矾锛?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 9 鏃ュ織澧炲己瀹屾垚锛沜argo clippy 閫氳繃 |
| 鎴戣鍘诲摢閲岋紵 | release 杩愯楠岃瘉鏃ュ織鏂囦欢瀹為檯鍐欏叆 + 7 澶╂竻鐞嗙敓鏁堬紙闇€绉疮鏃ュ織鎴栨墜鍔ㄦ祴璇曟竻鐞嗛€昏緫锛?|
| 鐩爣鏄粈涔堬紵 | 閫氳繃鏃ュ織鏂囦欢鑳藉畾浣嶉棶棰橈紝鑷姩娓呯悊 7 澶╁墠鏃ュ織 |
| 鎴戝鍒颁簡浠€涔堬紵 | 鐜版湁宸叉湁 setup_file_logging锛坉up2 stderr 鈫?鎸夊ぉ鏂囦欢锛夛紝鍙渶璋冮槇鍊?+ 閰嶇疆 env_logger 绾у埆鍗冲彲璁?log 瀹忕敓鏁堬紝鏃犻渶閲嶅啓鏃ュ織妗嗘灦锛沞nv_logger 榛樿 error 绾у埆浼氬悶鎺?info/warn锛屽繀椤绘樉寮?`default_filter_or("info")`锛沝up2 涔嬪墠鐨勬棩蹇楀啓鍘熷 stderr锛坮elease 涓㈠け锛夛紝鎵€浠ユ竻鐞嗚鏁拌鍦?dup2 鍚庣殑 banner 杈撳嚭 |
| 鎴戝仛浜嗕粈涔堬紵 | 闃舵 9锛氭竻鐞?14鈫? 澶╋紱env_logger 閰?info + 姣鏃堕棿鎴筹紱startup / SSH 鐢熷懡鍛ㄦ湡 / 蹇嵎鍛戒护 / delete_connection / channel_reader 琛ュ厖缁撴瀯鍖栨棩蹇?|

## 浼氳瘽锛?026-06-17 鈥?鏈湴缁堢锛堣繛鎺ユ湰鍦?PowerShell / CMD / WSL / 鑷畾涔?shell锛?

### 闃舵 10锛氭湰鍦扮粓绔叏鏍堬紙conn_type='local'锛?
- **鐘舵€侊細** 鍓嶇 complete锛坱sc 閫氳繃锛夛紱鍚庣浠ｇ爜 complete 浣?**鏈紪璇戦獙璇?*锛堢幆澧冩棤 rustup锛夛紱绔埌绔緟鐢ㄦ埛 `cargo build` + 杩愯
- **闇€姹傦細** 鍦?MyShell 閲岀洿杩炴湰鍦扮殑 PowerShell / CMD / WSL / 鑷畾涔?shell锛屼綔涓哄彲淇濆瓨鐨勮繛鎺ワ紙杩涙枃浠跺す銆佸甫 shell 閰嶇疆锛夛紝涓?SSH 缁堢浣撻獙涓€鑷?
- **璁捐鍐崇瓥锛堜笌鐢ㄦ埛纭锛夛細**
  - 鍏ュ彛褰㈡€侊細鏈湴缁堢浣滀负 `conn_type='local'` 鐨勪竴绉?`ConnectionConfig`锛屽鐢ㄧ幇鏈夎繛鎺ョ鐞?/ 鏂囦欢澶?/ 鍛戒护鍘嗗彶 / 蹇嵎鍛戒护鍏ㄥ 鈥斺€?涓嶆柊寤鸿〃銆佷笉鏂板啓绠＄悊 UI
  - 澶嶇敤 SSH 浜嬩欢閫氶亾锛氭湰鍦板悗绔?emit 鐜版湁鐨?`ssh_output` / `ssh_closed`锛宍TerminalPanel` 浜嬩欢璁㈤槄闆舵敼鍔紝鍙寜 `connType` 閫?connect/send/resize/disconnect 鍛戒护
  - vault 瑙ｉ攣淇濇寔鐜扮姸锛氭湰鍦拌繛鎺ヨ闅?connections 琛ㄥ姞瀵嗗瓨鍌紝鍒楀嚭闇€瑙ｉ攣锛坄get_connections` 鐨?DEK 闂ㄧ鑷姩瑕嗙洊锛夛紱`local_connect` 鏈韩涓嶉渶瑕佸瘑鐮?DEK
  - shell 閰嶇疆鍙屽瓧娈碉細`shell_path`锛堝彲鎵ц璺緞锛屾槑鏂囧垪锛? `shell_args`锛堝彲閫夊弬鏁帮級鈥斺€?涓嶇敤鍗曞瓧娈靛懡浠や覆锛岄伩鍏?`C:\Program Files\...` 绌烘牸琚?split 鐮村潖
  - 鎶€鏈€夊瀷锛歚portable-pty`锛圵indows ConPTY / Unix openpty锛寃ezterm 鍑哄搧锛夈€俙std::process` + pipe 鍥犳棤 PTY 琚惁鍐筹紙浜や簰寮?TUI / 棰滆壊 / resize 鍏ㄥ簾锛?
- **淇敼鏄庣粏锛?*
  - `src-tauri/Cargo.toml` 鈥?鍔?`portable-pty = "0.8"`
  - `src-tauri/src/local.rs`锛堟柊寤猴級鈥?`LocalCommand` 鏋氫妇锛圛nput/Resize/Disconnect锛屾湰鍦版棤 ZMODEM锛? `LocalSession { command_tx }` + `connect`锛歰penpty 鈫?spawn shell 鈫?**reader 闃诲绾跨▼**锛坄spawn_blocking`锛屽洜 portable-pty reader 鏄樆濉?`Read`锛塭mit `ssh_output`锛孍OF emit `ssh_closed` + 浠?map 绉婚櫎锛?*writer 浠诲姟**锛坅sync锛屾寔 master锛夊鐞?Input/Resize/Disconnect锛圖isconnect 鏃?`child.kill()`锛夈€俙send_input`/`resize_terminal`/`disconnect` 闀滃儚 ssh.rs
  - `src-tauri/src/db.rs` 鈥?`init_db` 鍔?`shell_path`/`shell_args` 鏄庢枃鍒楋紱`migrate_legacy_schema` 鍔犲箓绛夎縼绉伙紙`column_exists` 鎺㈡祴 + `ALTER TABLE ADD COLUMN`锛夛紱`get_all_connections`/`get_connection`/`save_connection` 涓夊 SELECT+鍏冪粍+INSERT 鍚屾鍔犲垪
  - `src-tauri/src/main.rs` 鈥?`mod local`锛沗ConnectionConfig` 鍔?`shell_path`/`shell_args`锛坄#[serde(default)]`锛夛紱`AppState.local_sessions` map + 鍒濆鍖栵紱`drain_all_sessions` 鍔?local 娓呯悊锛堝彂 `LocalCommand::Disconnect`锛夛紱4 鍛戒护 `local_connect`/`local_send`/`local_resize`/`local_disconnect`锛坄local_send` 澶嶇敤 256KB 杈撳叆涓婇檺锛? `generate_handler!` 娉ㄥ唽
  - `src/api.ts` 鈥?`ConnType` 鍔?`"local"`锛沗ConnectionConfig` 鍔?`shell_path`/`shell_args`锛坰nake_case锛屽尮閰嶅悗绔?serde 榛樿锛夛紱4 涓?wrapper
  - `src/App.tsx` 鈥?`handleConnect`/`handleReconnect`/`handleCloseTab` 鍔?`connType==='local'` 鍒嗘敮锛坄localConnect`/`localDisconnect`锛宒isplay name 鐢?`config.name`锛夛紱TerminalPanel 閫忎紶 `connType`
  - `src/components/TerminalPanel.tsx` 鈥?`connType` prop + `connTypeRef` + `sendTo`/`resizeTo` 鍒嗗彂锛堟寜 connType 閫?ssh_*/local_*锛夛紱浜嬩欢璁㈤槄 `onSshOutput`/`onSshClosed` 鍘熸牱澶嶇敤
  - `src/components/CommandBar.tsx` 鈥?`connType` prop + `sendFn` 鍒嗗彂锛?*淇**锛氬師鏈洿鎺?`sshSend`锛屾湰鍦?tab 鍛戒护浼氬彂閿欏悗绔紱鐜版寜 connType 閫?sshSend/localSend锛?
  - `src/components/ConnectionDialog.tsx` 鈥?`TYPE_OPTIONS` 鍔?local + `SHELL_PRESETS`锛坧wsh/powershell/cmd/wsl/git-bash锛夛紱`connType==='local'` 鏃惰〃鍗曞彧鏄剧ず 鍚嶇О/鍒嗙粍/shell 閫夋嫨+璺緞+鍙傛暟锛岄殣钘?host/port/auth/proxy锛沗handleSave` local 鍒嗘敮锛坔ost="" port=0 username=""锛宻hell_path 蹇呭～鏍￠獙锛?
  - `src/components/Sidebar.tsx` 鈥?`CONN_ICONS` 鍔?`local: 馃捇`
- **楠岃瘉锛?*
  - `npx tsc --noEmit` PASS锛堥娆?3 涓敊璇細ConnectionDialog 璇敤 `shell_path`/`shell_args` 鑰?api.ts 鎴戝厛鍐欐垚浜?camelCase `shellPath`/`shellArgs`锛涚粺涓€涓?snake_case 涓庡叾浠栧瓧娈典竴鑷村悗閫氳繃锛?
  - Rust `cargo build` **鏈墽琛?*锛堢幆澧冩棤 rustup锛夆€斺€?鏈€鍙兘鐨勮皟鏁寸偣鏄?portable-pty 0.8 鐨?`take_writer`/`spawn_command` 杩斿洖鐨?trait bound
- **閬楃暀椋庨櫓 / 寰呴獙璇侊細**
  - **ConPTY 缂栫爜**锛歐indows 涓?portable-pty 杈撳嚭鎸?shell 鐨?console codepage 璧帮紙pwsh=UTF-8 姝ｅ父锛沇indows PowerShell 5.1 鍦ㄤ腑鏂囩郴缁熷彲鑳?GBK 鈫?xterm 涓枃涔辩爜锛夈€倂1 emit 鍘熷瀛楄妭锛堜笌 SSH 涓€鑷达紝涓嶅仛杞崲锛夛紱濡傚疄娴嬩贡鐮侊紝鏀?pwsh 鎴栧湪 `local.rs` reader 鍔?`encoding_rs` 杞崲 / 娉ㄥ叆 `chcp 65001`
  - **Rust 鏈紪璇?*锛歱ortable-pty API 缁嗚妭銆乣CommandBuilder` 榛樿鐜/cwd 缁ф壙琛屼负闇€ `cargo build` + 杩愯纭
  - `shell_args` 绌虹櫧鍒嗗壊锛氬惈绌烘牸鐨勫崟涓弬鏁伴渶鐢ㄦ埛鑷姞寮曞彿锛坴1 闄愬埗锛宍local.rs` 娉ㄩ噴宸叉爣娉級
  - 鏈湴缁堢涓嶅弬涓庡箍鎾紙`getBroadcastTargets` 宸叉寜 `connType==='ssh'` 杩囨护锛変篃涓嶆樉绀?ServerInfoPanel锛堝悓鏍?ssh-only 鍒ゆ柇锛夆€斺€?鏃犻渶棰濆鎺掗櫎
- **棣栨绫诲瀷妫€鏌ヨ俯鍧戯細** `ConnectionConfig` 鍦?TS 渚т竴鐩寸敤 snake_case锛堝尮閰嶅悗绔?serde 榛樿锛屾棤 `rename_all`锛夛紝鏂板姞瀛楁鏃惰鐢?camelCase 瀵艰嚧涓嶄竴鑷达紱缁熶竴 snake_case 鍚?tsc 缁?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 10 鏈湴缁堢鍓嶇 complete锛坱sc 缁匡級锛涘悗绔唬鐮?complete 浣嗘湭缂栬瘧锛堟棤 rustup锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo build`锛堝彲鑳介渶璋?portable-pty API锛夆啋 `cargo tauri dev` 鈫?鏂板缓鏈湴杩炴帴锛坧wsh.exe锛夊弻鍑诲紑 tab 楠岃瘉鎵撳瓧/杈撳嚭/resize/鍏抽棴/缂栫爜 |
| 鐩爣鏄粈涔堬紵 | MyShell 鐩磋繛鏈湴 PowerShell/CMD/WSL/鑷畾涔?shell锛屼綔涓哄彲淇濆瓨鐨勮繛鎺ワ紝浣撻獙绛夊悓 SSH 缁堢 |
| 鎴戝鍒颁簡浠€涔堬紵 | 鏈湴缁堢涓?SSH 缁堢鍦ㄦ覆鏌撳眰瀹屽叏鍚屾瀯锛堥兘鏄?xterm + 瀛楄妭娴侊級锛屽樊寮傚彧鍦ㄤ笂娓告暟鎹簮 鈥斺€?鎶借薄鍑烘寜 connType 鍒嗗彂鍗冲彲闆舵垚鏈鐢紱portable-pty 鐨?reader 鏄樆濉?`Read`锛屽繀椤?`spawn_blocking` 鐙珛绾跨▼ + writer 浠诲姟鍙岀嚎锛堜笉鍚屼簬 SSH 鐨?select! 鍗曞惊鐜級锛汣ommandBar 鐩存帴璋?sshSend 鏄殣钘忕殑鍒嗗彂閬楁紡鐐癸紝鏂板鍚庣蹇呴』鍏ㄥ眬 grep 纭鎵€鏈?ssh_* 璋冪敤鐐?|
| 鎴戝仛浜嗕粈涔堬紵 | 闃舵 10锛欳argo + local.rs PTY 妯″潡锛沝b schema + 瀛楁锛沵ain AppState + 4 鍛戒护 + drain锛沘pi.ts 绫诲瀷 + wrapper锛汚pp/TerminalPanel/CommandBar 鎸?connType 鍒嗗彂锛汣onnectionDialog local 琛ㄥ崟 + shell 棰勮锛汼idebar 鍥炬爣 |

### 闃舵 10.1锛氱被鍨嬪浘鏍囦慨澶?+ 鍚姩鍛戒护 init_command锛?026-06-17锛?
- **鐘舵€侊細** 鍓嶇 complete锛坱sc 閫氳繃锛夛紱鍚庣浠ｇ爜 complete 鏈紪璇戯紱**鐗堟湰鍙?1.1.0 鈫?1.2.2**
- **闂1锛氭柊寤鸿繛鎺ョ被鍨嬮€夋嫨鍥炬爣鏄剧ず鏂规**
  - 鏍瑰洜锛歚ConnectionDialog` 鐨?`TYPE_OPTIONS` 閲?ssh/sftp/ftp 鐢?Nerd Font 绉佹湁鍖哄瓧绗︼紙`蟀枱`/`蟀墜`/`蟀垯`锛夛紝绯荤粺鏈 Nerd Font 灏辨覆鏌撴垚鏂规锛沴ocal 鐢?emoji锛坄馃捇`锛夋墍浠ユ甯?
  - 淇锛歚TYPE_OPTIONS` 鍥炬爣鏀?emoji锛屼笌 Sidebar `CONN_ICONS` 涓€鑷?鈥斺€?`馃枼锔廯/`馃搧`/`馃摛`/`馃捇`锛屽厤瀛椾綋璺ㄥ钩鍙?
- **闂2锛氭湰鍦扮粓绔墦寮€鍚庨粯璁ゆ墽琛屽懡浠わ紙init_command锛?*
  - 闇€姹傦細杩炴帴閲岄厤 `claude`锛屽紑 tab 鑷姩鎵ц
  - 璁捐锛歚ConnectionConfig` 鍔?`init_command`锛堟槑鏂囧垪锛岄€氱敤瀛楁鍏堟湰鍦扮敤锛夆啋 `local.rs` writer 浠诲姟 `take_writer` 鍚庣珛鍗虫敞鍏?`init_command + \r`锛圥TY stdin 缂撳啿锛宻hell 灏辩华鍚?echo + 鎵ц锛沗\r` 瑙﹀彂鎵ц锛屼笌 onData 杞彂 Enter 涓€鑷达級锛沗ConnectionDialog` 鏈湴琛ㄥ崟鍔犮€屽惎鍔ㄥ懡浠わ紙鍙€夛級銆嶈緭鍏?
  - 鏁版嵁灞傦細`db.rs` 鍔?`init_command TEXT` 鍒?+ 骞傜瓑杩佺Щ + `get_all`/`get`/`save` 涓夊 SQL 鍚屾锛沗main.rs`/`api.ts` `ConnectionConfig` 鍙岀鍔犲瓧娈?
  - 闄愬埗锛氬綋鍓嶆妸鏁存潯 `init_command` 褰?*鍗曡**鍛戒护娉ㄥ叆锛坱rim + `\r`锛夛紱澶氳鍛戒护鏆備笉鏀寔锛屽悗缁彲鎸?`\n` 鎷嗗垎渚濇娉ㄥ叆
- **鐗堟湰鍙凤細** `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` / `Cargo.lock`(myshell 鏉＄洰) 鍥涘 1.1.0 鈫?1.2.2锛沗backup.rs::APP_VERSION` 缁?`env!("CARGO_PKG_VERSION")` 鑷姩璺熼殢 Cargo.toml锛屾棤闇€鎵嬫敼
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛汻ust 寰?`cargo build`锛堝惈 portable-pty 缂栬瘧楠岃瘉锛?

## 浼氳瘽锛?026-06-18 鈥?瀹夎鍣ㄦ暟鎹垹闄ゅ畨鍏?/ 鏈湴缁堢娓叉煋涓庣紪鐮佷慨澶?

### 闃舵 11锛氬畨瑁呭櫒銆屽垹闄ゅ簲鐢ㄦ暟鎹€嶄簩娆＄‘璁?+ 鍗遍櫓鎻愮ず + 鐩寸櫧鏂囨
- **鐘舵€侊細** complete锛堥厤缃笌鑴氭湰灏变綅锛涙湰鏈烘棤 rustup锛屾湭璺?`cargo tauri build` 瀹為檯鎵撳寘楠岃瘉锛?
- **闂锛?* NSIS 鍗歌浇/鏇存柊娴佺▼鍕鹃€夈€屽垹闄ゅ簲鐢ㄧ▼搴忔暟鎹€嶏紙`deleteAppData`锛変細閫掑綊鍒?`$APPDATA\<bundle>` + `$LOCALAPPDATA\<bundle>` = 鏁翠釜 `connections.db`锛堝叏閮ㄨ繛鎺?/ 鏄庢枃瀵嗙爜 / 鍛戒护鍘嗗彶 / 蹇嵎鍛戒护 / 瀵嗛挜搴擄紝涓嶅彲鎭㈠锛夛紝浣嗗彧鏄竴涓棤璀﹀憡鐨勫閫夋 + 妯＄硦鏂囨锛屾瀬鏄撹鍒?
- **鏂规锛堜笉 fork 900 琛屾ā鏉匡紝鐢ㄥ畼鏂规満鍒讹級锛?*
  - **浜屾纭**锛歚installerHooks` 娉ㄥ叆 `NSIS_HOOK_PREUNINSTALL` 瀹?鈥斺€?鍦?`Section Uninstall` 寮€澶达紙鍒犳暟鎹箣鍓嶃€乣$UpdateMode`/`$DeleteAppDataCheckboxState` 宸插氨缁椂锛夋嫤鎴細浠呭綋鍕鹃€変笖闈炶嚜鍔ㄦ洿鏂版ā寮忓脊 `MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2`锛堥粯璁ゃ€屽惁銆嶃€乣/SD IDNO`锛夛紝鐐广€屽惁銆嶅垯缃?`$DeleteAppDataCheckboxState=0` 鍙栨秷鍒犻櫎
  - **鏂囨鐩寸櫧**锛歚customLanguageFiles`锛堝悎骞惰涔夛紝浠呰鐩?`deleteAppData` 涓€鏉★級鎶娿€屽垹闄ゅ簲鐢ㄧ▼搴忔暟鎹€嶁啋銆屽垹闄ゅ叏閮ㄥ簲鐢ㄦ暟鎹紙杩炴帴/瀵嗙爜/鍘嗗彶/瀵嗛挜搴擄紝涓嶅彲鎭㈠锛夈€?
- **淇敼鏄庣粏锛?*
  - `src-tauri/nsis/uninstall-confirm-hook.nsh`锛堟柊寤猴紝UTF-8 BOM锛夆€?`NSIS_HOOK_PREUNINSTALL` 瀹?+ 鍙岃鍗遍櫓鎻愮ず
  - `src-tauri/nsis/lang/SimpChinese.nsh`锛堟柊寤猴紝BOM锛? `English.nsh`锛堟柊寤猴級鈥?`deleteAppData` 瑕嗙洊
  - `src-tauri/tauri.conf.json` 鈥?`bundle.windows.nsis` 鍔?`installerHooks` + `customLanguageFiles`
- **楠岃瘉锛?* JSON 瑙ｆ瀽閫氳繃锛汵SIS 閽╁瓙璇硶/BOM/鍙嶆枩鏉犵画琛岄€愰」鏍稿锛涘疄鏈洪獙璇佸緟 `cargo tauri build` 鍚庤鐩栧畨瑁呪啋鍗歌浇椤靛嬀閫夆啋搴旇璀﹀憡妗嗛粯璁ゃ€屽惁銆?
- **鏈哄埗纭锛?* `NSIS_HOOK_PREUNINSTALL` 鐢辨ā鏉?`Section Uninstall` 椤堕儴 `!insertmacro`锛堟湰鍦扮敓鎴?installer.nsi L747-748 瀹炶瘉锛夛紱`customLanguageFiles` 涓?Rust 灞傛寜閿悕鍚堝苟锛圢SIS 涓嶅厑璁稿悓鍚?LangString 閲嶅锛屾晠蹇呬负鍚堝苟鑰岄潪 include 鍙犲姞锛屼粎闇€鍐欒鐩栭」锛夆€斺€?渚濇嵁 Tauri v2 瀹樻柟鏂囨。 + 閰嶇疆鍙傝€?

### 闃舵 12锛氭湰鍦扮粓绔覆鏌撲笌缂栫爜淇锛堝瓧浣?/ TERM 鐜 / 瀛椾綋鍙厤缃?/ UTF-8 纭寲锛?
- **鐘舵€侊細** 鍓嶇 complete锛坄npx tsc --noEmit` PASS锛夛紱鍚庣浠ｇ爜 complete 鏈紪璇戯紙鏃?rustup锛?
- **闂锛?* 鏈湴缁堢锛坄conn_type='local'`锛変粠 MyShell 鎵撳紑鏃躲€屽瓧浣撴牱寮忎贡鐮併€嶁€斺€斺憼 xterm `fontFamily` 鍙垪鍩虹瀛椾綋锛圕ascadia Code/Fira Code鈥︼級锛屾棤 Nerd Font锛屾彁绀虹 powerline/鍥炬爣瀛楀舰锛圤h My Posh 绛夛級娓叉煋鎴愯眴鑵愬潡锛涒憽 `local.rs` spawn 缁ф壙鐖惰繘绋嬬幆澧冩棤 `TERM`/`COLORTERM`锛屾彁绀虹寮曟搸鍙兘闄嶇骇锛涒憿 cmd / Windows PowerShell 5.1 鍦?zh-CN 鍚?GBK 鈫?涓枃涔辩爜锛堥樁娈?0 閬楃暀锛?
- **鏂规锛?*
  - **瀛椾綋**锛歺term 瀛椾綋鏍堟敼涓?Nerd Font 浼樺厛锛圕askaydiaCove/Cascadia Code NF/MesloLGM/JetBrainsMono/FiraCode/Hack Nerd Font锛? 鍩虹瀛椾綋鍥為€€
  - **鐜**锛歚CommandBuilder::env` 澹版槑 `TERM=xterm-256color` / `COLORTERM=truecolor` / `TERM_PROGRAM=MyShell`锛坧ortable-pty `env()` 涓哄姞鎬ц鐩栵紝PATH/profile 鐓у父缁ф壙 鈥斺€?宸叉煡鏂囨。纭锛?
  - **瀛椾綋鍙厤缃?*锛氭柊澧?localStorage 璁剧疆锛堟部鐢ㄤ富棰?閰嶈壊鍚屼竴濂楁寔涔呭寲妯″紡锛夆€斺€?鐢ㄦ埛濉叆鏈満宸茶瀛椾綋鍚嶅嵆鐢熸晥锛堟棤闇€鍐呯疆瀛椾綋锛岃鐢?Nerd Font 鐨勭敤鎴峰繀鐒跺凡鑷锛?
  - **UTF-8 纭寲**锛歚local.rs` 鍚姩鏃舵寜 shell 鍚嶆敞鍏?UTF-8 鍓嶅锛堝啓鍦?init_command 涔嬪墠锛夆€斺€?cmd锛歚@chcp 65001>nul`锛汸owerShell 5.1锛歚[Console]::{Output,Input}Encoding=[Text.Encoding]::UTF8; chcp 65001 > $null`锛沺wsh / bash / zsh / wsl 涓嶅鐞嗭紙鏈氨 UTF-8锛?
- **淇敼鏄庣粏锛?*
  - `src/components/TerminalPanel.tsx` 鈥?瀛椾綋鏍堢Щ鑷冲叡浜父閲忥紱`useTerminalFont` 鍙栧瓧浣擄紱鏂板 `useEffect` 鐑洿鏂?`term.options.fontFamily`锛堝凡寮€缁堢鍏嶉噸寮€锛?
  - `src/themes.ts` 鈥?鍔?`STORAGE_KEY_TERMINAL_FONT` + `TERMINAL_FONT_DEFAULT_STACK`
  - `src/hooks/useTerminalFont.ts`锛堟柊寤猴級鈥?localStorage 璇?鍐?+ 瑙ｆ瀽 `fontFamily`锛堥€変腑瀛椾綋 + 榛樿鍥為€€鏍堬級
  - `src/components/SettingsPanel.tsx` 鈥?鏂板銆岀粓绔瓧浣撱€峉ection锛圛nput + 甯歌 Nerd Font 鎻愮ず锛?
  - `src-tauri/src/local.rs` 鈥?`use std::path::Path`锛涙柊澧?`shell_utf8_prelude()`锛堟寜 file_stem 鍖归厤锛岃８鍚?鍏ㄨ矾寰勭殕璁わ級锛沗connect` 璁＄畻 prelude 骞?move 杩?writer 浠诲姟锛宍take_writer` 鍚庡厛浜?init_command 鍐欏叆
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛汻ust 寰?`cargo tauri dev`锛堣缃潰鏉挎敼瀛椾綋鈫掑凡寮€/鏂板紑缁堢瀛楀舰鍗冲嚭锛涙湰鍦拌繛 cmd/powershell.exe 涓枃涓嶅啀涔辩爜锛?
- **閬楃暀 / 鎻愰啋锛?* PowerShell 5.1 鍚姩浼氬洖鏄句竴琛岀紪鐮佸懡浠わ紙浜や簰寮?PS 鏃犳硶骞插噣鎶戝埗锛屽彲鎺ュ彈锛夛紱棣栧抚鎻愮ず绗︼紙profile 鐢ㄦ棫缂栫爜缁樺埗锛夊彲鑳界暐鐟曠柕锛屾敞鍏ュ悗鍏ㄩ儴 UTF-8

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 11/12 complete锛堝墠绔?tsc 缁匡紝Rust 鏈紪璇戯級锛涘凡鎸夋柊瑙勫悓姝ユ洿鏂?progress.md + README.md |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri build` 楠岃瘉瀹夎鍣ㄤ簩娆＄‘璁?+ `cargo tauri dev` 楠岃瘉瀛椾綋/缂栫爜 |
| 鐩爣鏄粈涔堬紵 | 鍗歌浇鏁版嵁鍒犻櫎闃茶鍒狅紱鏈湴缁堢瀛椾綋/缂栫爜涓庣湡缁堢涓€鑷?|
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri NSIS 閽╁瓙锛坄NSIS_HOOK_PREUNINSTALL`锛? `customLanguageFiles`锛堝悎骞惰涔夛級鑳藉湪涓?fork 妯℃澘鐨勫墠鎻愪笅鏀瑰嵏杞借涓轰笌鏂囨锛沺ortable-pty `CommandBuilder::env` 鍔犳€ц鐩栵紙闀滃儚 std `Command`锛夛紱鏈湴缁堢涔辩爜鏄€屽瓧浣撶己 Nerd Font 绉佹湁鍖哄瓧褰?+ 缂?TERM 鐜 + 闈?UTF-8 shell銆嶄笁鍥犲彔鍔?|
| 鎴戝仛浜嗕粈涔堬紵 | 闃舵 11锛氬嵏杞介挬瀛?+ 璇█涓茶鐩?+ tauri.conf 鎺ョ嚎锛涢樁娈?12锛歂erd Font 榛樿鏍?+ TERM/COLORTERM + 瀛椾綋鍙厤缃紙hook/璁剧疆闈㈡澘/鐑洿鏂帮級+ cmd/PS5.1 UTF-8 鍓嶅 |

### 闃舵 13锛氱粓绔瓧浣撯€斺€旂郴缁熷瓧浣撲笅鎷夐€夋嫨 + 鎸夎繛鎺ュ崟鐙鐩栵紙2026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坄npx tsc --noEmit` PASS锛夛紱鍚庣浠ｇ爜 complete 鏈紪璇戯紙鏃?rustup锛宖ont-kit 棣栨鎷夊彇 + DB 杩佺Щ寰?`cargo build` 楠岃瘉锛?
- **闇€姹傦紙鐢ㄦ埛鍙嶉锛夛細** 鈶?瀛椾綋璁剧疆涓嶈鎵嬭緭锛屽簲鏌ヨ绯荤粺鍙敤瀛椾綋鍋氫笅鎷夐€夋嫨锛涒憽 涓嶅悓杩炴帴鍙兘闇€瑕佷笉鍚屽瓧浣擄紙鍙闂€?甯哥洴鐨勭敓浜ф満鏇村ぇ瀛楀彿/浠呮煇浜涜繛鎺ョ敤 Nerd Font锛夛紝甯屾湜鎸夌粓绔崟鐙瀛椾綋
- **鍐崇瓥锛堜笌鐢ㄦ埛纭涓や釜鍒嗗弶锛夛細** 瀛椾綋鏋氫妇鐢?**font-kit 鍚庣鐪熷疄鏋氫妇**锛堟渶璐村悎"鏌ヨ绯荤粺瀛椾綋"锛岃法骞冲彴锛涗唬浠锋槸鏂板 1 涓緷璧栵級锛涙寜杩炴帴瑕嗙洊瀛?**鏁版嵁搴撳垪**锛堥殢杩炴帴璧帮紝瀵煎嚭/瀵煎叆澶囦唤涓€璧峰甫璧帮紝娌跨敤 shell_path/init_command 鍚屾骞傜瓑杩佺Щ锛?
- **璁捐锛?* 澶嶇敤 `FontField` 缁勪欢锛坕nput + 鍘熺敓 `<datalist>`锛屾ā鍧楃骇缂撳瓨鍏变韩涓€娆?fetch锛屽け璐ラ檷绾т负绾墜杈擄級锛涘叏灞€璁剧疆 + 鎸夎繛鎺ヨ鐩栭兘鐢ㄥ畠锛汿erminalPanel 瑙ｆ瀽 `override ?? global` 鐑洿鏂?
- **淇敼鏄庣粏锛?*
  - `src-tauri/Cargo.toml` 鈥?鍔?`font-kit = "0.14"`
  - `src-tauri/src/fonts.rs`锛堟柊寤猴級鈥?`list_system_fonts` 鍛戒护锛歚SystemSource::new().all_families()` 鈫?鎺掑簭鍘婚噸锛屽け璐ヨ繑绌?
  - `src-tauri/src/db.rs` 鈥?`connections` 鍔?`terminal_font TEXT` 鍒?+ 骞傜瓑杩佺Щ + `get_all`/`get`/`save` 涓夊 SQL锛圫ELECT 鏈熬杩藉姞 index 20銆乼uple銆乻truct銆両NSERT 鍒?VALUES/params锛?
  - `src-tauri/src/main.rs` 鈥?`ConnectionConfig` 鍔?`terminal_font: Option<String>`锛坄#[serde(default)]`锛夛紱`mod fonts`锛沗generate_handler!` 娉ㄥ唽 `list_system_fonts`
  - `src/api.ts` 鈥?`ConnectionConfig.terminal_font?` + `listSystemFonts()` wrapper
  - `src/hooks/useTerminalFont.ts` 鈥?鎶藉嚭瀵煎嚭 `resolveFontStack(primary?)`锛堥€変腑瀛椾綋浼樺厛 + 榛樿鍥為€€鏍堬級锛宧ook 涓庢寜杩炴帴瑕嗙洊鍏辩敤
  - `src/components/FontField.tsx`锛堟柊寤猴級鈥?input + datalist锛宍useId` 闃插瀹炰緥 id 鍐茬獊锛屾ā鍧楃骇 fetch 缂撳瓨
  - `src/components/SettingsPanel.tsx` 鈥?鍏ㄥ眬瀛椾綋 `Input` 鈫?`FontField`
  - `src/components/ConnectionDialog.tsx` 鈥?`terminalFont` state锛泂sh+local 鏄剧ず銆岀粓绔€岶ieldGroup锛團ontField锛岀暀绌?鍏ㄥ眬锛夛紱`handleSave` 涓ゅ垎鏀紙local / ssh路sftp路ftp锛夐兘鍐?`terminal_font`
  - `src/components/TerminalPanel.tsx` 鈥?`fontOverride?` prop锛沗fontFamily = fontOverride ? resolveFontStack(fontOverride) : globalFontFamily`锛涙棦鏈?live-update effect 鑷姩瑕嗙洊
  - `src/App.tsx` 鈥?涓ゅ `<TerminalPanel>` 浼?`fontOverride={connections.find(...)?.terminal_font}`
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛汻ust 寰?`cargo tauri dev`锛坒ont-kit 棣栨缂栬瘧 + 楠岃瘉鏋氫妇杩斿洖 + DB 杩佺Щ + 瀛椾綋瑕嗙洊鐢熸晥锛?
- **閬楃暀 / 椋庨櫓锛?* font-kit 涓烘柊渚濊禆锛圵indows 鐢?DirectWrite 鏋氫妇锛屾瀯寤哄簲鍙潬锛屼絾鏈満鏃犳硶棰勭紪璇戦獙璇侊紱鑻?build 鎶ラ敊鏈€鍙兘鍦ㄦ渚濊禆锛夛紱DB get_all/get/save 鐨勫垪绱㈠紩鏀瑰姩闇€ `cargo build` 纭鏃?off-by-one

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 13 鍓嶇 complete锛坱sc 缁匡級锛涘悗绔?font-kit + DB 杩佺Щ鏈紪璇?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 楠岃瘉锛氳缃?杩炴帴瀵硅瘽妗嗗瓧浣撲笅鎷夊垪鍑虹郴缁熷瓧浣擄紱鎸夎繛鎺ヨ瀛椾綋鍚庤 tab 鐢熸晥 |
| 鐩爣鏄粈涔堬紵 | 瀛椾綋浠庣郴缁熷瓧浣撻€夋嫨锛堥潪鎵嬭緭锛夛紱鏀寔鎸夎繛鎺ュ崟鐙鐩栧瓧浣?|
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri NSIS 涔嬪鍙堜竴涓?鍓嶇瑕佺殑鑳藉姏鍦ㄥ悗绔灇涓惧啀璧?IPC"妯″紡锛堝瓧浣撴灇涓剧敤 font-kit锛屽墠绔浂鏉冮檺寮圭獥锛夛紱DB 鍔犲垪瑕佸悓姝ユ敼 SELECT/row.get index/tuple 瑙ｆ瀯/struct/INSERT 浜斿锛孲ELECT 鏈熬杩藉姞鏂板垪鍙繚鎸佹棦鏈?index 涓嶅姩锛坈reated_at 浠?19锛屾柊鍒?20锛夐檷浣?off-by-one 椋庨櫓锛泋term fontFamily 鍙?live mutation锛屾寜杩炴帴瑕嗙洊澶嶇敤鍚屼竴 effect |
| 鎴戝仛浜嗕粈涔堬紵 | 闃舵 13锛歠ont-kit 鏋氫妇鍛戒护 + DB terminal_font 鍒?杩佺Щ + 绫诲瀷鍙岀 + FontField 缁勪欢 + 璁剧疆/杩炴帴瀵硅瘽妗嗘帴鍏?+ TerminalPanel override 瑙ｆ瀽 + App 閫忎紶 |

### 闃舵 14锛氬璇濇銆岀偣鍑婚伄缃╁嵆鍏抽棴銆嶈瑙︿慨澶嶏紙2026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坄npx tsc --noEmit` PASS锛?
- **闂锛堢敤鎴峰弽棣堬級锛?* 璁剧疆 / 蹇嵎鍛戒护鐣岄潰涓庢柊寤鸿繛鎺ョ晫闈㈠瓨鍦ㄥ悓涓€涓瘺鐥呪€斺€旂偣鍒版搷浣滄锛堝璇濇鍐呭锛夊鐨勯伄缃╁尯鍩燂紝鐣岄潰鐩存帴閫€鍑恒€傝缃?蹇嵎鍛戒护杩欑被闀胯〃鍗曡瑙︿唬浠烽珮锛堝凡濉唴瀹瑰叏涓級銆傛柊寤鸿繛鎺ワ紙`ConnectionDialog`锛夊叾瀹炲凡鏃犳闂锛坥verlay 鏈寕 `onClick`锛夛紝鍏朵綑寮圭獥鏈榻愩€?
- **鏍瑰洜锛?* 杩欎簺寮圭獥鐨勯伄缃╁眰 `<div>` 涓婃寕浜?`onClick={onClose}`锛屽唴瀹瑰鍣ㄥ啀 `stopPropagation` 闃绘柇鈥斺€旇繖鏄€岀偣閬僵鍏抽棴銆嶆爣鍑嗘ā寮忋€傞暱琛ㄥ崟涓嶉€傚悎锛岄渶缁熶竴涓恒€屽彧鏈夊叧闂寜閽?/ 鍙栨秷鎸夐挳鍏抽棴銆嶃€?
- **鏂规锛?* 绉婚櫎鎵€鏈夐伄缃╁眰鐨?`onClick={onClose}`锛涚偣鍑婚伄缃╀笉鍐嶅叧闂紝蹇呴』璧板叧闂寜閽€傚唴閮?`stopPropagation` 涓€寰嬩繚鐣欙紙鏃犲壇浣滅敤銆侀槻寰℃€э級锛涙墍鏈夋寜閽殑 `onClick` 涓嶅姩銆?
- **淇敼鏄庣粏锛? 澶勯伄缃╋級锛?*
  - `src/components/SettingsPanel.tsx` 鈥?涓婚潰鏉?overlay锛坺Index 2000锛? `Dialog` 瀛愮粍浠?overlay锛坺Index 2100锛岃嚜瀹氫箟涓婚寮圭獥鐢級涓ゅ `onClick={onClose}` 绉婚櫎
  - `src/components/QuickCommandsPanel.tsx` 鈥?闈㈡澘 overlay `onClick={onClose}` 绉婚櫎
  - `src/components/PassphraseDialog.tsx` 鈥?overlay `onClick={onClose}` 绉婚櫎
  - `src/components/PasswordVerifyDialog.tsx` 鈥?overlay `onClick={onClose}` 绉婚櫎
- **宸叉牳鏌ヤ笉鍙樻洿锛堟棤姝ら棶棰樻垨涓嶉€傜敤锛夛細**
  - `ConnectionDialog.tsx` 鈥?overlay 鏈氨鏃?`onClick`锛堟纭弬鐓ф牱鏈級
  - `MasterPasswordGate.tsx` 鈥?鍚姩涓诲瘑鐮侀棬锛屾棤 overlay `onClick`锛堜笉搴旇交鏄撳叧闂級
  - `App.tsx:653` 鈥?浠呮槸杩炴帴澶辫触鎻愮ず妗嗙殑銆屽叧闂€嶆寜閽紙闈為伄缃╋級锛屼繚鐣?
  - `Sidebar.tsx:801` 鈥?鍙抽敭鑿滃崟閬僵锛岀偣绌虹櫧鍏抽棴鑿滃崟鏄湡鏈涜涓猴紝涓嶅姩
- **楠岃瘉锛?* `npx tsc --noEmit` PASS
- **闄勶細闃舵 13 build 鏀跺熬** 鈥?`src-tauri/src/main.rs` `generate_handler!` 閲?`list_system_fonts` 鏀逛负闄愬畾璺緞 `fonts::list_system_fonts`锛堝懡浠ゅ畾涔夊湪 `fonts.rs` 鑰岄潪 main.rs锛岄渶妯″潡闄愬畾锛夛紱`cargo check` 宸茬豢锛岄樁娈?13 鍚庣鍙紪璇?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 14 complete锛堝璇濇璇Е鍏抽棴淇锛宼sc 缁匡級锛涢樁娈?13 build 宸叉敹灏撅紙cargo check 缁匡級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 楠岃瘉锛氭墦寮€璁剧疆 / 蹇嵎鍛戒护 / 涓诲瘑鐮?/ 瀵嗙爜楠岃瘉寮圭獥锛岀偣閬僵涓嶅啀鍏抽棴锛屽彧鑳界敤鍏抽棴/鍙栨秷鎸夐挳 |
| 鐩爣鏄粈涔堬紵 | 鎵€鏈夊璇濇鐐规搷浣滄澶栦笉鎰忓閫€鍑猴紝琛屼负涓庢柊寤鸿繛鎺ヤ竴鑷?|
| 鎴戝鍒颁簡浠€涔堬紵 | 銆宱verlay `onClick={onClose}` + content `stopPropagation`銆嶆槸鐐归伄缃╁叧闂殑鏍囧噯妯″紡锛屼絾闀胯〃鍗曡瑙︿唬浠烽珮搴旂鐢紱鎵归噺鏀规椂闈?`position:fixed;inset:0` 鐨勯伄缃╁尯鍒嗐€宱verlay 鐨?onClick銆嶄笌銆屾寜閽殑 onClick銆嶏紝閫愪釜鐢?zIndex 涓婁笅鏂囧敮涓€瀹氫綅閬垮厤璇垹鎸夐挳 |
| 鎴戝仛浜嗕粈涔堬紵 | 4 涓粍浠跺叡 5 澶勯伄缃╃Щ闄?`onClick={onClose}`锛涢『甯︿慨闃舵 13 build 閿欒锛坄fonts::` 闄愬畾璺緞锛?|

### 闃舵 15锛氬瓧浣撻€夋嫨鈥斺€旀ā绯婃悳绱笅鎷?+ 鏍峰紡缁熶竴锛?026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坄npx tsc --noEmit` PASS锛?
- **闇€姹傦紙鐢ㄦ埛鍙嶉锛夛細** 鈶?瀛椾綋閫夋嫨鏀寔妯＄硦鎼滅储锛岄€氳繃涓嬫媺妗嗛€夋嫨锛涒憽 鐜板湪瀛椾綋涓嬫媺妗嗗緢涓戯紝缁熶竴鏍峰紡
- **闂锛堝師瀹炵幇锛夛細** `FontField` 鐢ㄥ師鐢?`<datalist>`鈥斺€斺憼 娴忚鍣ㄩ粯璁や笅鎷夋牱寮忔棤娉曞畾鍒讹紝涓庡簲鐢?Catppuccin 涓婚瀹屽叏涓嶆惌锛堜笐锛夛紱鈶?鍙仛鍓嶇紑/瀛愪覆鍖归厤锛岃緭鍏ヤ腑闂磋瘝鏃犳硶杩囨护锛岃皥涓嶄笂銆屾ā绯婃悳绱€?
- **鏂规锛?* 閲嶅啓涓鸿嚜瀹氫箟 combobox锛坧rops 绛惧悕 `value/onChange/placeholder` 涓嶅彉锛宍SettingsPanel` / `ConnectionDialog` 璋冪敤澶勯浂鏀瑰姩锛?
  - **妯＄硦鍖归厤**锛氭煡璇㈡寜绌烘牸鍒嗚瘝锛屾瘡涓?token锛堝ぇ灏忓啓涓嶆晱鎰燂級閮介渶鍑虹幇鍦ㄥ瓧浣撳悕涓€侀『搴忔棤鍏?鈫?`nerd mono` 鍛戒腑 `JetBrainsMono Nerd Font Mono`
  - **`value` / `query` 鍒嗙**锛氳繃婊ょ敤鐙珛 `query`锛屽凡閫夊瓧浣撻噸鏂拌仛鐒︿粛鏄剧ず瀹屾暣鍒楄〃锛屼笉浼氳鑷韩鍊艰繃婊ゆ帀锛涢€変腑 / 澶栭儴娓呯┖鏃堕噸缃?`query`
  - **鍖归厤鐗囨楂樹寒**锛氱粨鏋滀腑鎶婂懡涓?token 鍔犵矖 + 鍔犳繁鑹诧紝妯＄硦鍛戒腑涓€鐩簡鐒?
  - **缁熶竴鏍峰紡**锛氳緭鍏ユ澶嶇敤椤圭洰杈撳叆妗嗘牱寮?+ focus 钃濈幆锛坄--accent-primary` + `0 0 0 3px --accent-primary-muted`锛夛紱涓嬫媺闈㈡澘鐢?`--bg-elevated` / `--border-emphasis` / `--shadow-xl` / `--radius-md`锛岄€夐」楂樹寒鐢?`--accent-primary-muted` + `--accent-primary`锛屼笌 TypeSelector / 鎸夐挳閫変腑鎬佷竴鑷达紱鍙充晶鍔?`鈻綻 鎻愮ず鍙笅鎷?
  - **浜や簰**锛氶敭鐩?`鈫戔啌` 绉诲姩楂樹寒銆乣Enter` 閫変腑銆乣Esc` 鍏抽棴锛沨over 鍚屾楂樹寒锛涚偣鍑婚€変腑銆傞€夐」 `onMouseDown preventDefault` 闃叉 input blur 鍏堝叧鎺変笅鎷?
  - **鎬ц兘**锛歚MAX_RESULTS = 200` + 瓒呴噺鎴柇鎻愮ず锛涘瓧浣撳垪琛ㄦā鍧楃骇缂撳瓨锛堟部鐢級
  - 鍔犺浇涓?/ 鏃犲尮閰?绌虹姸鎬佹彁绀猴紝鑷敱鏂囨湰杈撳叆浠嶅彲鐢紙绯荤粺瀛椾綋鏋氫妇鍙兘涓嶅叏锛?
- **淇敼鏄庣粏锛?*
  - `src/components/FontField.tsx` 鈥?鏁翠綋閲嶅啓锛坉atalist 鈫?combobox + `renderHighlighted` 鐗囨楂樹寒宸ュ叿锛?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛涘緟 `cargo tauri dev` 瀹炴祴锛氳缃?/ 杩炴帴瀵硅瘽妗嗗瓧浣撹緭鍏ヨЕ鍙戞ā绯婅繃婊ゃ€佷笅鎷変富棰樹竴鑷淬€侀敭鐩樺彲瀵艰埅銆佸尮閰嶅瓧鍔犵矖
- **閬楃暀 / 闄愬埗锛?* 涓嬫媺鐢?`relative + absolute`锛屼綅浜庤缃潰鏉?/ 杩炴帴瀵硅瘽妗嗙殑 `overflowY: auto` 鍐呭鍖哄唴锛涘瓧浣撳瓧娈佃创杩戝彲瑙嗗尯搴曢儴鏃朵笅鎷夊彲鑳借瑁佸壀锛堝唴瀹瑰尯鍙粴鍔ㄧ湅鍒帮級銆備袱澶勫疄闄呬娇鐢ㄤ綅缃紙璁剧疆涓銆佽繛鎺ュ璇濇琛ㄥ崟涓婇儴锛夌┖闂村厖瓒筹紝鏆備笉寮曞叆 portal/fixed 瀹氫綅锛涜嫢鍙嶉瑁佸壀鍐嶅崌绾?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 15 complete锛堝瓧浣?combobox锛宼sc 缁匡級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 楠岃瘉锛氳缃?/ 杩炴帴瀵硅瘽妗嗗瓧浣撴杈撳叆鍗虫ā绯婅繃婊ゃ€佷笅鎷夋牱寮忕粺涓€銆侀敭鐩?鈫戔啌/Enter/Esc 鍙敤 |
| 鐩爣鏄粈涔堬紵 | 瀛椾綋妯＄硦鎼滅储 + 涓嬫媺閫夋嫨 + 涓庡簲鐢ㄧ粺涓€鐨勪笅鎷夋牱寮?|
| 鎴戝鍒颁簡浠€涔堬紵 | 鍘熺敓 `<datalist>` 涓嶅彲瀹氬埗鏍峰紡涓斿彧鍓嶇紑鍖归厤锛沜ombobox 蹇呴』 `value`/`query` 鍒嗙锛屽惁鍒欏凡閫夊瓧浣撹仛鐒︽椂浼氳鑷韩鍊艰繃婊ゆ垚鍙墿鑷繁锛涢€夐」鐐瑰嚮鐢?`onMouseDown preventDefault` 闃绘 input 鍏?blur 鍏充笅鎷夛紝鏄?combobox 缁忓吀鍧?|
| 鎴戝仛浜嗕粈涔堬紵 | `FontField` 閲嶅啓锛氭ā绯婂尮閰嶏紙澶?token 浠绘剰椤哄簭锛? value/query 鍒嗙 + 鍖归厤鐗囨楂樹寒 + 涓婚鍖栦笅鎷?+ 閿洏/hover/鐐瑰嚮浜や簰 + 鎴柇涓庣┖鐘舵€?|

### 闃舵 16锛氭湰鍦拌繛鎺ャ€屼互绠＄悊鍛樿繍琛屻€嶏紙鏁翠綋鎻愭潈鏂规锛夛紙2026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坄npx tsc --noEmit` PASS锛夛紱鍚庣 complete锛坄cargo check` PASS锛寃inapi 鏂板 features 缂栬瘧閫氳繃锛?
- **闇€姹傦紙鐢ㄦ埛锛夛細** 鏈湴鐨勮繛鎺ユ槸鍚﹀彲浠ユ敮鎸佺鐞嗗憳杩愯锛?
- **鎶€鏈皟鐮旓紙涓轰綍涓嶅崟杩炴帴鎻愭潈锛夛細** 鏈湴 shell 缁?`portable-pty` 鐨?`openpty()` + `spawn_command()` 鍚姩锛岀户鎵?MyShell 瀹屾暣鎬х骇鍒紙IL锛夈€傚崟杩炴帴鎻愭潈鐨勭‖绾︽潫锛氣憼 `portable-pty`/`CommandBuilder` 璧?`CreateProcessW`锛屾棤鎻愭潈閫夐」锛涒憽 鏍囧噯鎻愭潈 `ShellExecute("runas")` 鏃犳硶鎸?ConPTY锛圕onPTY 瑕佸厛 `CreatePseudoConsole` 鍐嶇敤甯?`hPC` 鐨?`STARTUPINFOEX`锛夛紱鈶?medium-IL 杩涚▼鏃犳硶鎶?ConPTY attach 鍒?high-IL shell锛堝畬鏁存€х骇鍒殧绂伙級銆傜粨璁猴細瑕佽 elevated shell 璺戣繘鎴戜滑鐨?ConPTY锛?*ConPTY 蹇呴』鍦ㄥ凡鎻愭潈杩涚▼鍐呭垱寤?*鈥斺€斿嵆鍗曡繛鎺ユ彁鏉冮渶鍙﹁捣 elevated helper 杩涚▼ + 璺ㄥ畬鏁存€х骇鍒懡鍚嶇閬?IPC 杞彂锛屽伐绋嬮噺澶с€佹瘡娆″脊 UAC銆佺淮鎶ら噸锛涜繛 Windows Terminal 閮藉彧寮€鐙珛 elevated 绐楀彛鑰岄潪鍗曡繘绋嬫贩璺?IL銆?
- **鍐崇瓥锛堢敤鎴烽€夊畾鏂规 A 路 鏁翠綋鎻愭潈锛夛細** 浠ョ鐞嗗憳韬唤閲嶅惎 MyShell锛屾彁鏉冨悗鎵€鏈夋湰鍦拌繛鎺ヨ嚜鍔ㄨ幏寰楃鐞嗗憳鏉冮檺銆傛渶灏忎唬浠疯鐩?鍋跺皵瑕佺鐞嗗憳璺戝懡浠?鐨勭湡瀹為渶姹傘€?
- **鏂规锛?*
  - 鏂板 `elevation.rs`锛?
    - `is_elevated()` 鈥?Windows锛歚OpenProcessToken` + `GetTokenInformation(TokenElevation)`锛涢潪 Windows锛歚geteuid()==0`锛坋xtern C 澹版槑锛屽厤 libc 渚濊禆锛?
    - `restart_as_admin()` 鈥?Windows锛歚ShellExecuteW(verb="runas")` 瑙﹀彂 UAC锛岃繑鍥?HINSTANCE 鈮?32 涓洪敊璇紙1223=ERROR_CANCELLED 鐢ㄦ埛鍙栨秷锛夛紱闈?Windows锛歴tub 杩斿洖"鏆備笉鏀寔"
  - `main.rs`锛歚is_elevated` / `restart_as_admin` 涓や釜 `#[tauri::command]`锛涘悗鑰呮垚鍔熷悗 `app.exit(0)` 瑙﹀彂 `ExitRequested` 鈫?`drain_all_sessions` 浼橀泤鎺掔┖鍐嶉€€鍑猴紝elevated 鏂板疄渚嬬敱绯荤粺鍦?UAC 鍚庣嫭绔嬪惎鍔?
  - 鍓嶇锛歚api.ts` 鍔?`isElevated()` / `restartAsAdmin()`锛沗SettingsPanel` 鏂板銆岎煕★笍 绠＄悊鍛樻潈闄愩€峉ection锛堢姸鎬?chip锛氭娴嬩腑 / 鉁?宸叉槸绠＄悊鍛?/ 褰撳墠鏅€氱敤鎴?+ 鏈彁鏉冩椂銆屼互绠＄悊鍛橀噸鍚€嶆寜閽?+ 璀﹀憡鏉★級锛沗ConnectionDialog` 鏈湴 shell 鎻愮ず鍔犲紩瀵艰
  - 渚濊禆锛歸inapi 鍗囦负 Windows-only 鐩存帴渚濊禆锛坒eatures锛歚processthreadsapi`/`securitybaseapi`/`winnt`/`handleapi`/`shellapi`/`winuser`锛夆€斺€斿師涓?portable-pty 闂存帴渚濊禆锛屽０鏄庣洿鎺ヤ緷璧栦互渚挎墜鍐?elevation FFI
- **淇敼鏄庣粏锛?*
  - `src-tauri/src/elevation.rs`锛堟柊寤猴級
  - `src-tauri/Cargo.toml` 鈥?`[target.'cfg(windows)'.dependencies] winapi = { ..., features=[...] }`
  - `src-tauri/src/main.rs` 鈥?`mod elevation` + 2 鍛戒护 + `generate_handler!` 娉ㄥ唽
  - `src/api.ts` 鈥?`isElevated` / `restartAsAdmin`
  - `src/components/SettingsPanel.tsx` 鈥?import `confirm`/`isElevated`/`restartAsAdmin`锛沗elevated`/`restartBusy` state + 鍔犺浇 effect锛沗handleRestartAdmin`锛坄confirm` 浜屾纭 鈫?`restartAsAdmin`锛屽彇娑?UAC 闈欓粯銆佸叾浠栭敊璇?alert锛夛紱绠＄悊鍛樻潈闄?Section
  - `src/components/ConnectionDialog.tsx` 鈥?鏈湴 shell 璇存槑杩藉姞绠＄悊鍛樺紩瀵?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛坵inapi features 榻愬叏锛宍myshell` 缂栬瘧閫氳繃锛?
- **閬楃暀 / 闄愬埗锛?*
  - 绮掑害涓哄叏灞€锛氶噸鍚悗鎵€鏈夎繛鎺ユ彁鏉冿紝闈炲崟杩炴帴锛涢噸鍚涪澶卞綋鍓?tab锛堝凡鍦ㄨ鍛婃潯 + 浜屾纭璇存槑锛?
  - 姣忔閲嶅惎寮逛竴娆?UAC
  - 闈?Windows锛歚restart_as_admin` 涓?stub锛屽墠绔寜閽湪闈?root 鏃朵粛鍙浣嗙偣鍑绘姤"褰撳墠骞冲彴鏆備笉鏀寔"锛堥」鐩?Windows 浼樺厛锛屾湭鍋氬钩鍙扮骇闅愯棌锛夛紱濡傞渶鍙湪鍓嶇鎸?`navigator.platform` 闅愯棌

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 16 complete锛堢鐞嗗憳閲嶅惎锛涘墠绔?tsc 缁?+ 鍚庣 cargo check 缁匡級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 鈫?璁剧疆闈㈡澘鐪嬨€岀鐞嗗憳鏉冮檺銆嶇姸鎬?鈫?鏈彁鏉冪偣銆屼互绠＄悊鍛橀噸鍚€嶁啋 UAC銆屾槸銆嶁啋 鏂板疄渚嬬鐞嗗憳杩愯 鈫?寮€鏈湴杩炴帴鎵ц闇€绠＄悊鍛樺懡浠?|
| 鐩爣鏄粈涔堬紵 | 鏈湴杩炴帴鏀寔浠ョ鐞嗗憳韬唤杩愯 shell锛堟暣浣撴彁鏉冩柟妗堬級 |
| 鎴戝鍒颁簡浠€涔堬紵 | ConPTY + UAC 瀹屾暣鎬х骇鍒殧绂讳娇鍗曡繛鎺ユ彁鏉冨繀椤?elevated helper + 璺?IL IPC锛圵indows Terminal 涔熷彧寮€鐙珛 elevated 绐楀彛锛夆啋 鏁翠綋鎻愭潈鎬т环姣旀渶楂橈紱`ShellExecuteW("runas")` 杩斿洖 HINSTANCE鈮?2 涓洪敊锛?223=鐢ㄦ埛鍙栨秷锛夛紱winapi 鐩存帴渚濊禆闇€鎸夌敤鍒扮殑 API 绮剧‘鍒?features |
| 鎴戝仛浜嗕粈涔堬紵 | `elevation.rs`锛坕s_elevated + restart_as_admin锛? winapi Windows-only 渚濊禆 + main.rs 涓ゅ懡浠わ紙閲嶅惎鍚?app.exit 瑙﹀彂 drain锛? api.ts + SettingsPanel 绠＄悊鍛?Section + ConnectionDialog 寮曞 |

### 闃舵 17锛歷1.3.3 缁煎悎浼樺寲锛堝巻鍙茶繃婊?/ 淇濆瓨鎸夐挳鎮诞 / 鐗堟湰鍙峰崟涓€婧?/ 鍥炬爣閲嶇敓鎴愶級锛?026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坱sc PASS锛夛紱鍚庣 complete锛坈argo check PASS锛宍myshell v1.3.3`锛夛紱鍥炬爣鍏ㄥ閲嶇敓鎴愶紙tauri icon from source-a.svg锛?
- **鍥涢」鏀瑰姩锛?*
  1. **鍘嗗彶鍛戒护搴熷懡浠よ繃婊?*锛氱敤鎴峰弽棣堝巻鍙查噷鏈?"A"銆?AD" 杩欑被璇Е鍗?鍙屽瓧姣嶆棤鏁堝懡浠ゃ€傚湪 `add_command_history` 杈圭晫锛坢ain.rs锛岃鐩?TerminalPanel + CommandBar 涓や釜璋冪敤鍏ュ彛锛夊姞 `is_junk_command`锛歵rim 鍚庝粎鐢?a/d 瀛楃缁勬垚锛堝ぇ灏忓啓涓嶆晱鎰燂級鈫?杩斿洖 Ok(0) 涓嶅叆搴撱€傗殸锔?姝よ鍒欎篃鍖归厤 `dd`锛圠inux 甯哥敤锛夛紝宸插悜鐢ㄦ埛鎻愮ず锛屽闇€鍙姞鐧藉悕鍗?
  2. **鏂板缓杩炴帴淇濆瓨鎸夐挳鎮诞**锛欳onnectionDialog 搴曢儴銆屽彇娑?淇濆瓨銆峟ooter 鍔?`position: sticky; bottom: 0`锛堝崱鐗囨槸 overflowY auto 婊氬姩瀹瑰櫒锛夛紝闀胯〃鍗曚笉鐢ㄦ粴鍒板簳涔熻兘鐐逛繚瀛?
  3. **鐗堟湰鍙峰崟涓€婧?*锛歚Cargo.toml [package] version` 涓哄敮涓€鎵嬪姩婧愶紱`tauri.conf.json` 鍒犻櫎 version 瀛楁锛圱auri v2 鑷姩璇?Cargo.toml锛夛紱鏂板 `scripts/sync-version.mjs` 鍦?`npm run build`锛坱auri build 鐨?beforeBuildCommand锛夊墠鑷姩鎶?version 鍚屾鍒?package.json + package-lock.json锛屼害鍙?`npm run version:sync` 鎵嬪姩銆備互鍚庡崌鐗堝彧鏀?Cargo.toml 涓€澶?
  4. **鎵撳寘鍥炬爣閲嶇敓鎴?*锛氱敤鎴蜂笂浼犵殑鍥炬爣鍗冲綋鍓?Aurora Prompt锛坰ource-a.svg 娓叉煋锛夈€傜敤鐭㈤噺婧?`source-a.svg`锛?024 viewBox锛夎窇 `npx tauri icon` 閲嶆柊鐢熸垚鍏ㄥ鈥斺€攊con.ico锛坋xe锛夈€乮con.icns锛坢ac锛夈€佸悇灏哄 PNG銆丼quare*Logo/StoreLogo锛圢SIS 瀹夎鍣級銆乮OS/Android锛岀‘淇濆叏濂楅珮娓呬竴鑷?
- **鐗堟湰鍙凤細** 1.2.2 鈫?1.3.3锛圕argo.toml / package.json / package-lock锛泃auri.conf.json 鏃?version锛岀敱 Cargo.toml 椹卞姩锛?
- **淇敼鏄庣粏锛?*
  - `src-tauri/src/main.rs` 鈥?`add_command_history` 鏀圭敤 trimmed + `is_junk_command` 杩囨护锛涙柊澧?`is_junk_command` 鍑芥暟
  - `src/components/ConnectionDialog.tsx` 鈥?footer 鍔?sticky bottom / flexShrink / zIndex
  - `src-tauri/Cargo.toml` / `package.json` / `package-lock.json` 鈥?version 鈫?1.3.3
  - `src-tauri/tauri.conf.json` 鈥?鍒犻櫎 version 瀛楁
  - `scripts/sync-version.mjs`锛堟柊寤猴級鈥?Cargo.toml 鈫?package.json/lock 鍚屾鑴氭湰
  - `package.json` scripts 鈥?build 鍓嶇疆 sync-version锛屾柊澧?version:sync
  - `src-tauri/icons/*` 鈥?tauri icon 浠?source-a.svg 閲嶆柊鐢熸垚鍏ㄥ
- **楠岃瘉锛?* tsc PASS锛沜argo check PASS锛坴1.3.3锛夛紱sync-version.mjs 杩愯 OK锛?already at 1.3.3"锛夛紱tauri icon 鍏ㄥ鐢熸垚 OK锛坕con.ico/icns/png/Square*Logo/iOS/Android 鍧?11:21 鏇存柊锛?
- **閬楃暀 / 鎻愰啋锛?* 鈶?鍘嗗彶杩囨护 dd 椋庨櫓锛堣涓婏紝寰呯敤鎴风‘璁ゆ槸鍚﹀姞鐧藉悕鍗曪級锛涒憽 tauri.conf.json 鍒?version 渚濊禆 Tauri v2 鑷姩璇?Cargo.toml锛堝畼鏂硅涓猴紝寰?`tauri build` 瀹炴祴 version 鏄剧ず锛夛紱鈶?鐪嬫柊 exe 鍥炬爣闇€閲嶆柊 `cargo tauri build`锛學indows 鍙兘瑕佹竻鍥炬爣缂撳瓨

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 17 complete锛坴1.3.3 鍥涢」浼樺寲锛宼sc + cargo check 缁匡紝鍥炬爣閲嶇敓鎴愶級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri build` 楠岃瘉锛氱増鏈?1.3.3銆乪xe 鍥炬爣銆佸畨瑁呭櫒锛沗cargo tauri dev` 楠岃瘉鍘嗗彶鍛戒护杩囨护 + 淇濆瓨鎸夐挳鎮诞 |
| 鐩爣鏄粈涔堬紵 | 杩囨护鍘嗗彶搴熷懡浠わ紱淇濆瓨鎸夐挳甯搁┗锛涚増鏈彿鍙敼涓€澶勶紱鎵撳寘鍥炬爣缁熶竴楂樻竻 |
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri v2 `tauri.conf.json` 鐨?version 鍙渷鐣ャ€佽嚜鍔ㄨ Cargo.toml锛堝崟涓€婧愬叧閿級锛沗npx tauri icon` 鎺ュ彈 SVG 鐭㈤噺婧愮敓鎴愬叏骞冲彴鍥炬爣锛堣緭鍑洪粯璁ゅ湪 tauri.conf.json 鏃佺殑 icons/锛夛紱`position: sticky; bottom: 0` 鍦?overflowY auto 瀹瑰櫒鍐呰 footer 甯搁┗涓斾笉鑴辩娴侊紙鏃犻伄鎸★級锛涘巻鍙茶繃婊ゆ斁鍦ㄧ郴缁熻竟鐣岋紙鍛戒护灞傦級鍙鐩栨墍鏈夎皟鐢ㄥ叆鍙?|
| 鎴戝仛浜嗕粈涔堬紵 | main.rs 鍘嗗彶杩囨护 + ConnectionDialog footer sticky + 鐗堟湰鍙峰崟涓€婧愶紙Cargo.toml 鍞竴 / tauri.conf.json 鍒?/ sync 鑴氭湰锛? tauri icon 鍏ㄥ閲嶇敓鎴?+ 鐗堟湰鍙?1.3.3 |

### 闃舵 17 琛ワ細NSIS 鎵撳寘 BOM 淇锛?026-06-18锛?
- **鐜拌薄锛?* `cargo tauri build` 鍦?NSIS 闃舵澶辫触锛歚Invalid command: "锘?"` 鈫?`!include: error in script: SimpChinese.nsh on line 1` 鈫?`aborting creation process`銆?
- **鏍瑰洜锛?* `src-tauri/nsis/lang/SimpChinese.nsh` 涓?`src-tauri/nsis/uninstall-confirm-hook.nsh` 鏂囦欢澶村甫 **UTF-8 BOM锛圲+FEFF锛?*锛孨SIS 涓嶈 BOM锛屾妸 BOM 褰撳懡浠よВ鏋愩€俙English.nsh` 鏃?BOM 鏁呮甯搞€傛綔浼忛棶棰樷€斺€斾箣鍓嶅悇闃舵鍙窇 cargo check / dev锛屾湭璺戝畬鏁?NSIS 鎵撳寘锛屾湭鏆撮湶銆?
- **淇锛?* 鍘绘帀杩欎袱涓?`.nsh` 鐨?BOM锛堜繚鐣?UTF-8 鍐呭锛汵SIS Unicode 妯″紡鎸?UTF-8 璇讳腑鏂囷級銆?
- **楠岃瘉锛?* 閲嶆柊 `npm run tauri:build` 鎴愬姛锛岀敓鎴?`MyShell_1.3.3_x64-setup.exe`锛涙枃浠跺悕 1.3.3 璇佹槑 tauri.conf.json 鍒?version 鍚?Tauri 姝ｇ‘鍙栬嚜 Cargo.toml锛堝崟涓€婧愭柟妗堢敓鏁堬級銆?
- **鏁欒锛?* NSIS 鐨?`.nsh`锛坈ustomLanguageFiles / installerHooks锛夊繀椤?**鏃?BOM**锛涗互鍚庢柊澧?缂栬緫 .nsh 娉ㄦ剰缂栬緫鍣ㄥ埆瀛?BOM銆?

### 闃舵 18锛氭墦鍖呰鍛婃竻鐞?+ 鏈湴 PowerShell 閫忔槑鑳屾櫙娓叉煋淇锛?026-06-18锛?
- **鐘舵€侊細** 鍓嶇 complete锛坱sc PASS锛夛紱鎵撳寘楠岃瘉閫氳繃锛圡yShell_1.3.3_x64-setup.exe锛屼袱 warning 娑堝け锛?
- **涓夐」鏀瑰姩锛?*
  1. **bundle identifier**锛歚com.myshell.app` 鈫?`com.myshell.client`锛堟秷闄?Tauri "ends with `.app`" macOS 鍐茬獊璀﹀憡锛夈€傚壇浣滅敤锛氭柊 identifier = 鏂?app 鏍囪瘑锛汥B/杩炴帴/瀵嗙爜/鍘嗗彶涓嶅彉锛堝瓨 `dirs/myshell`锛屼笉渚濊禆 identifier锛夛紱WebView2 localStorage锛堜富棰?瀛椾綋锛夊彲鑳介噸缃紱鏃х増锛坈om.myshell.app锛夐渶鎵嬪姩鍗歌浇
  2. **api.ts 鍔ㄦ€佸鍏?*锛欰pp.tsx `await import("./api")` 鏀逛负闈欐€?`import { deleteConnection }`锛堟秷闄?vite "dynamically imported but also statically imported" 璀﹀憡锛宐undle 635鈫?32KB锛?
  3. **鏈湴 PowerShell 杈撳叆瀛楁瘝涔辫烦 + 褰卞搷鑳屾櫙**锛氭牴鍥犫€斺€旂粓绔鑳屾櫙鍥炬椂 `allowTransparency: true`锛寈term 榛樿 **canvas renderer 鍦ㄩ€忔槑妯″紡涓嬮噸缁樹笉娓呴櫎鏃у儚绱?* 鈫?杈撳叆瀛楃娈嬪奖鍙犲姞锛?涔辫烦"锛? 娈嬪奖绯婂湪鑳屾櫙鍥句笂锛?褰卞搷鑳屾櫙"锛夛紝鏈湴 ConPTY 杈撳嚭鏇存槗瑙﹀彂銆備慨澶嶏細鍔?`@xterm/addon-webgl`锛屽惎鐢?**WebGL renderer**锛堟瘡甯у畬鏁撮噸缁橈紝閫忔槑鍚堟垚骞插噣鏃犳畫褰憋級锛屽け璐?fallback canvas
- **淇敼鏄庣粏锛?*
  - `src-tauri/tauri.conf.json` 鈥?identifier 鈫?`com.myshell.client`
  - `src/App.tsx` 鈥?deleteConnection 鏀归潤鎬?import锛沷nDelete 鍘绘帀 `await import`
  - `package.json` 鈥?鍔?`@xterm/addon-webgl@^0.18`
  - `src/components/TerminalPanel.tsx` 鈥?import WebglAddon锛沗term.open` 鍚?`loadAddon(new WebglAddon())`锛坱ry/catch fallback锛?
- **楠岃瘉锛?* tsc PASS锛沗npm run tauri:build` 鎴愬姛锛屼袱 warning 娑堝け锛沇ebGL 寰?dev/build 瀹炴祴涔辫烦鏄惁娑堥櫎
- **閬楃暀锛?* 鈶?鍥炬爣闂寰呮緞娓呪€斺€旂敤鎴峰弽棣?setup.exe 鍥炬爣"浠嶆槸鏃х殑"锛屼絾 `source-a.svg` 鏈氨鏄綋鍓?chevron锛岄噸鏂扮敓鎴愬悓娆炬棤鍙樺寲锛岄渶鐢ㄦ埛纭鏈熸湜鐨勫浘鏍囪璁℃垨鎺掓煡 Windows 鍥炬爣缂撳瓨锛涒憽 PowerShell 涔辫烦淇寰呯敤鎴峰疄娴嬬‘璁わ紙鑻?WebGL 涓嶈В鍐冲啀鎺掓煡 cols/ConPTY 鏃跺簭锛?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 18 complete锛坕dentifier + vite 璀﹀憡娓呯悊宸查獙璇侊紱PowerShell WebGL 淇寰呭疄娴嬶級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴鏈湴 PowerShell 杈撳叆涓嶅啀涔辫烦/绯婅儗鏅紱纭 setup.exe 鍥炬爣闂锛堢紦瀛?or 闇€鎹㈡簮锛?|
| 鐩爣鏄粈涔堬紵 | 娑堥櫎涓や釜鎵撳寘 warning锛涗慨澶嶆湰鍦?PowerShell 閫忔槑鑳屾櫙娓叉煋锛涙緞娓呭浘鏍?|
| 鎴戝鍒颁簡浠€涔堬紵 | xterm `allowTransparency` + 榛樿 canvas renderer 閫忔槑妯″紡閲嶇粯涓嶆竻鍍忕礌锛堟畫褰憋級锛岄€忔槑鑳屾櫙搴旂敤 WebGL renderer锛汿auri identifier 涓嶅簲浠?`.app` 缁撳熬锛坢acOS 鍐茬獊锛夛紱鏀?identifier 涓嶅奖鍝?dirs/myshell 涓嬬殑 DB锛屼絾 WebView2 localStorage 浼氶噸缃紱vite 闈欐€?鍔ㄦ€佹贩鐢ㄥ鍏ュけ鍘诲垎鍧楁剰涔?|
| 鎴戝仛浜嗕粈涔堬紵 | identifier 鏀?com.myshell.client + App.tsx 鍔ㄦ€佹敼闈欐€?+ TerminalPanel 鍔?WebGL renderer + @xterm/addon-webgl 渚濊禆 |

### 闃舵 18 琛ワ細NSIS 瀹夎鍖咃紙setup.exe锛夊浘鏍囦慨澶嶏紙2026-06-18锛?
- **鐜拌薄锛?* 鐢ㄦ埛鍙嶉 setup.exe锛堝畨瑁呭寘锛夊浘鏍囦笉鏄?chevron锛屼絾搴旂敤鍥炬爣锛坢yshell.exe锛夋甯搞€?
- **鏍瑰洜锛?* 搴旂敤鍥炬爣璧?`bundle.icon`锛坕con.ico锛屾甯革級锛涗絾 **NSIS 瀹夎鍣ㄥ浘鏍囨槸鐙珛鐨?`bundle.windows.nsis.installerIcon` 瀛楁**锛屼箣鍓嶆湭閰嶇疆 鈫?Tauri 鐢ㄥ唴缃粯璁ゅ畨瑁呭櫒鍥炬爣銆俰nstaller.nsi 閲?`MUI_ICON "${INSTALLERICON}"`锛岃€?INSTALLERICON 鏈寚鍚戠敤鎴峰浘鏍囥€?
- **淇锛?* `tauri.conf.json` 鐨?nsis 鍧楀姞 `"installerIcon": "icons/icon.ico"`锛坰ource-a.svg 娓叉煋鐨?chevron锛夈€?
- **楠岃瘉锛?* rebuild 鍚?installer.nsi:39 `!define INSTALLERICON "F:\...\src-tauri\icons\icon.ico"`锛宻etup.exe锛圡yShell_1.3.3_x64-setup.exe锛夊祵鍏?chevron 鍥炬爣銆?
- **閬楃暀锛?* 璧勬簮绠＄悊鍣ㄨ嫢浠嶆樉绀烘棫鍥炬爣锛屾槸 Windows 鍥炬爣缂撳瓨锛坰etup.exe 鍚屽悕缂撳瓨锛夆€斺€斿彸閿睘鎬х湅鐪熷疄鍥炬爣锛屾垨娓?`%localappdata%\IconCache.db` + 閲嶅惎璧勬簮绠＄悊鍣ㄣ€?

### 闃舵 18 琛?2锛歂SIS 瀹氬埗鏁翠綋鍥為€€鍒板垵濮嬶紙2026-06-18锛?
- **鐜拌薄锛?* 鐢ㄦ埛鍙嶉鍗歌浇椤甸潰绌虹櫧銆佸畨瑁呮渶缁堥〉鏈夐棶棰橈紙闃舵 11 鐨?NSIS 瀹氬埗 + 闃舵 18 琛ョ殑 installerIcon 寮曡捣锛夈€?
- **鍐崇瓥锛?* 鐢ㄦ埛瑕佹眰鎶婂畨瑁?鍗歌浇鍙樻洿鍥為€€鍒板垵濮嬬姸鎬併€?
- **鍥為€€锛?*
  - `tauri.conf.json` nsis 鍧楁仮澶嶅埌 HEAD 鍒濆 鈥斺€?绉婚櫎 `installerIcon` / `installerHooks` / `customLanguageFiles`锛屼粎淇濈暀 `installMode` / `languages` / `displayLanguageSelector`
  - 鍒犻櫎 `src-tauri/nsis/` 鐩綍锛坲ninstall-confirm-hook.nsh + lang/SimpChinese.nsh + lang/English.nsh锛屽潎涓洪樁娈?11 鏂板鐨?untracked 鏂囦欢锛?
- **淇濈暀锛堥潪瀹夎/鍗歌浇椤甸潰鍙樻洿锛夛細** 鐗堟湰鍙峰崟涓€婧愶紙Cargo.toml 椹卞姩锛宼auri.conf.json 鏃?version 瀛楁锛夈€乮dentifier `com.myshell.client`
- **楠岃瘉锛?* rebuild 鎴愬姛锛孧yShell_1.3.3_x64-setup.exe 鐢熸垚锛沬nstaller.nsi 鏃?`customLanguageFile` / `uninstall-confirm` 寮曠敤锛坄NSIS_HOOK_*` 鏄?Tauri 榛樿妯℃澘鐨勯挬瀛愭鏌ョ偣锛屽畯鏈畾涔夋晠璺宠繃锛夆啋 鍗歌浇椤?/ 瀹夎鏈€缁堥〉鎭㈠ Tauri 榛樿锛堜笉鍐嶇┖鐧?閿欒锛?
- **褰卞搷锛?* 鈶?setup.exe 鍥炬爣鍥炲埌 Tauri 榛樿锛坕nstallerIcon 宸茬Щ闄わ級锛涒憽 鍗歌浇涓嶅啀鏈?鍒犻櫎搴旂敤鏁版嵁"浜屾纭閽╁瓙锛涒憿 璇█涓茬敤 Tauri 榛樿锛堜笉鍐嶈鐩?deleteAppData 鏂囨锛?
- **鍚庣画锛堝悓鏃ワ紝installerIcon 鍗曠嫭鍔犲洖锛夛細** 搴旂敤鎴疯姹傚彧鎭㈠瀹夎鍣ㄥ浘鏍?鈥斺€?nsis 鍧楀姞鍥?`"installerIcon": "icons/icon.ico"`锛堥挬瀛?/ 璇█瑕嗙洊淇濇寔鍥為€€锛夈€傞獙璇侊細rebuild 鎴愬姛锛宨nstaller.nsi `INSTALLERICON 鈫?icons/icon.ico`锛宍customLanguageFile | uninstall-confirm` 璁℃暟 = 0 鈫?瀹夎鍣ㄥ浘鏍?= chevron锛屽嵏杞介〉 / 瀹夎椤典粛 Tauri 榛樿锛堜笉绌虹櫧锛?

### 闃舵 18 琛?3锛歐ebGL 閫忔槑鑳屾櫙淇锛堣儗鏅浘鍙橀粦锛夛紙2026-06-18锛?
- **鐜拌薄锛?* 闃舵 18 鍔?WebGL renderer 鍚庯紝鐢ㄦ埛鍙嶉鑳屾櫙鍥惧彉榛戯紙鐪嬩笉鍒板疄闄呰儗鏅浘锛夈€?
- **鏍瑰洜锛?* 缁堢閫忔槑鑳屾櫙涔嬪墠鐢?`theme.background = "transparent"` 瀛楃涓层€俢anvas renderer 璁よ繖涓?CSS 鍏抽敭瀛楋紙閫忔槑锛夛紝浣?**WebGL renderer 瑙ｆ瀽棰滆壊澶辫触**锛宑learColor 鍥為€€鍒颁笉閫忔槑榛戯紝鐩栦綇鑳屾櫙鍥惧眰銆?
- **淇锛?* 閫忔槑鑹叉敼 `rgba(0, 0, 0, 0)`锛坅lpha=0锛學ebGL 鑳芥纭В鏋愶級銆備袱澶勶細鍒濆 Terminal 鐨?`theme` + live theme effect 鐨?`currentBg`銆?
- **楠岃瘉锛?* tsc PASS锛涘緟 build/dev 瀹炴祴锛歐ebGL 閫忔槑锛堣儗鏅浘閫忓嚭锛? 姣忓抚閲嶇粯锛堟棤娈嬪奖 / 涔辫烦锛夈€?
- **鏁欒锛?* xterm theme 鑹茶鐢?WebGL / canvas 閮借兘瑙ｆ瀽鐨勬牸寮忥紙rgba / hex锛夛紝鍒敤 `"transparent"` 鍏抽敭瀛椻€斺€攃anvas 瀹瑰繊銆亀ebgl 涓嶈銆?

### 闃舵 19锛氭湰鍦扮粓绔緭鍏ュ瓧绗﹁烦鍑?+ 鑳屾櫙宸︾Щ锛坈ols 琚?padding 姹℃煋锛夛紙2026-06-19锛?
- **鐜拌薄锛?* 鐢ㄦ埛鍙嶉鏈湴缁堢锛圥owerShell / ConPTY锛夎緭鍏ュ懡浠ゆ椂瀛楃浼氳烦鍑虹晫闈紝骞跺嚭鐜拌儗鏅乏绉汇€傝繖姝ｆ槸闃舵 18 WebGL 淇锛堥€忔槑鑳屾櫙娈嬪奖锛変箣鍚?*浠嶆畫鐣?*鐨勭棁鐘垛€斺€旈樁娈?18 閬楃暀椤光憽銆岃嫢 WebGL 涓嶈В鍐冲啀鎺掓煡 cols / ConPTY 鏃跺簭銆嶉鍒ょ殑 cols 鏂瑰悜銆?
- **鏍瑰洜锛?* FitAddon锛園xterm/addon-fit 0.10.x锛塦proposeDimensions` 璇?`getComputedStyle(瀹瑰櫒).width`锛坆order-box 瀹藉害锛夊綋鍙敤瀹藉害锛屽啀闄や互 cellWidth 寰?cols銆傝€?`global.css` 鍏ㄥ眬 `* { box-sizing: border-box }` + `TerminalPanel.tsx` 鐨?**xterm 瀹瑰櫒鑷韩甯?`padding: 4`** 鈫?瀹瑰櫒 `.width`锛坆order-box锛屽惈 padding锛夆墵 `.xterm` 瀹為檯濉厖鐨?content box锛堝噺 8px锛夈€侳itAddon 鍥犳澶氱畻绾?8px 鈮?**澶?1 鍒?*銆倄term 鎶婅繖涓亸澶х殑 cols 缁?`localResize`/`sshResize` 鍙戠粰 PTY锛孭SReadLine 姣忔鎸夐敭鎸夊亸澶?cols 鍏ㄨ閲嶇粯 + 缁濆鍏夋爣瀹氫綅 `\x1b[<n>G` 鈫?鏈€鍚庝竴鍒楃敾鍒?canvas 澶栵紙瀛楃璺冲嚭锛夈€侀噸缁樻竻闄よ寖鍥翠笌鍙鍖洪敊浣嶏紙鑳屾櫙宸︾Щ锛夈€傛湰鍦?PowerShell/ConPTY 涓?PSReadLine 瀵?cols 鏈€鏁忔劅鏁呮渶鏄庢樉锛圫SH bash readline 鍚屾牱鍙楀奖鍝嶏紝鍙槸琛ㄧ幇涓嶅悓锛夈€?
- **淇锛?* 鎶?`padding: 4` 浠?xterm 瀹瑰櫒绉诲埌澶栧眰 wrapper div锛涘灞傚悓鏃惰 `background: terminalTheme.background`锛堥潪鑳屾櫙鍥炬ā寮忥級淇濊瘉 4px 鍐呯缉鏃犵紳锛堜笉闇?App 搴曡壊锛夛紱瀹瑰櫒鑷韩鍘?padding锛宍.xterm` 骞插噣濉弧 content box锛堟鏃?content box == border box锛夛紝FitAddon 璇诲埌鐨勫搴?== `.xterm` 鐪熷疄娓叉煋瀹藉害 鈫?cols 绮剧‘銆傝儗鏅浘妯″紡涓?wrapper 閫忔槑 + 鑳屾櫙鍥惧眰 `inset:0` 浠嶉摵婊?padding box锛堣儗鏅摵鍒拌竟缂樸€佹枃瀛楀唴缂╋級銆?
- **淇敼鏄庣粏锛?* `src/components/TerminalPanel.tsx` 鈥斺€?澶栧眰 wrapper div锛坄.terminal-bg-transparent` 閭ｅ眰锛夊姞 `padding: 4` + `background`锛沗containerRef` div 绉婚櫎 `padding: 4` 骞跺姞璇︾粏娉ㄩ噴瑙ｉ噴涓轰綍涓嶈兘鍦ㄦ灞傚姞 padding銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆傚緟 `cargo tauri dev` 瀹炴祴锛氭湰鍦扮粓绔緭鍏ラ暱鍛戒护涓嶅啀璺冲嚭銆佷笉鍐嶈儗鏅乏绉伙紱SSH 缁堢琛屽熬瀵归綈浜﹀簲鏇村噯銆?
- **閬楃暀锛?* 鏈湴缁堢浠?80脳24 鍚姩锛坢ain.rs `local_connect` 鍐欐锛夛紝闈?mount 鍚?100ms fit + 棣栧抚杈撳嚭鍚庡啀 fit 涓ゆ resize 鍚屾鐪熷疄 cols锛涙湰娆″彧淇?cols 璁＄畻锛堣鍏跺噯纭級锛屾椂搴忛€昏緫鏈韩鏈敼銆傝嫢鏋佷釜鍒満鏅粛鍋跺彂鎶栧姩锛屽啀鑰冭檻鏀?initial cols 鎴栧姞 debounce銆?
- **鍏宠仈锛?* 闃舵 18 WebGL renderer锛堣В鍐抽€忔槑娈嬪奖锛? 鏈 cols 淇锛屼袱鑰呭彔鍔犳墠褰诲簳瑙ｅ喅"杈撳叆涔辫烦 / 绯婅儗鏅?鈥斺€斿墠鑰呯閫忔槑閲嶇粯锛屽悗鑰呯鍒楀銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 19 complete锛堟湰鍦扮粓绔?cols 姹℃煋淇锛宼sc 缁匡紝寰?dev 瀹炴祴锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴鏈湴 PowerShell锛氳緭鍏ラ暱鍛戒护涓嶈烦鍑恒€佷笉鑳屾櫙宸︾Щ锛涢『甯﹂獙璇?SSH 琛屽熬瀵归綈 |
| 鐩爣鏄粈涔堬紵 | 褰诲簳瑙ｅ喅闃舵 18 WebGL 涔嬪悗浠嶆畫鐣欑殑"瀛楃璺冲嚭 + 鑳屾櫙宸︾Щ"鈥斺€旇繖娆＄殑鏍瑰洜鏄?cols 琚鍣?padding 姹℃煋 |
| 鎴戝鍒颁簡浠€涔堬紵 | FitAddon 璇荤殑鏄?`getComputedStyle(瀹瑰櫒).width`锛坆order-box锛屽惈 padding锛夛紱鍏ㄥ眬 `box-sizing:border-box` 涓嬶紝鎵胯浇 `.xterm` 鐨勫鍣?*缁濅笉鑳藉甫 padding**锛堜細琚畻杩涘垪瀹斤級锛岃瑙夊唴缂╁簲鏀惧灞?wrapper锛?杈撳叆涔辫烦"鍙湁澶氬眰鏍瑰洜锛堥樁娈?18 閫忔槑娈嬪奖 / 闃舵 19 cols 鍋忓樊锛夛紝闇€閫愬眰鎺掓煡 |
| 鎴戝仛浜嗕粈涔堬紵 | TerminalPanel.tsx padding 浠?xterm 瀹瑰櫒绉诲埌澶栧眰 wrapper锛? 澶栧眰琛ヨ儗鏅壊淇濇棤缂濓級+ 娉ㄩ噴 + tsc 楠岃瘉 |

### 闃舵 20锛氭湰鍦扮粓绔緭鍏ュ瓧绗﹂敊浣嶏紙浜岋級鈥斺€?瀛椾綋鍔犺浇绔炴€佽嚧 cols 婕傜Щ锛?026-06-19锛?
- **鐜拌薄锛?* 闃舵 19 padding 淇鍚庣敤鎴蜂粛鍙嶉锛氭湰鍦扮粓绔緭鍏ユ椂"鍦ㄨ儗鏅梺杈硅緭鍏ュ瓧绗︼紝鎶婅儗鏅尋鍒板乏杈?锛?*闅忔満鍑虹幇**銆?
- **瀹氫綅锛圓skUserQuestion 閿佺幆澧冿級锛?* 鈶?璁剧疆浜嗚嚜瀹氫箟鑳屾櫙鍥撅紙鈫?璧?allowTransparency + WebGL 閫忔槑璺緞锛夆憽 PowerShell 7 (pwsh) 鈶?闅忔満 / 涓嶇‘瀹氥€?
- **鏍瑰洜锛?* pwsh 鐨?PSReadLine **姣忔鎸夐敭閮界敤缁濆鍏夋爣瀹氫綅锛坄\x1b[<col>G`锛夐噸缁樻暣涓緭鍏ヨ**銆備竴鏃?xterm 鐨?cols 涓?PTY 璁や负鐨勫垪鏁颁笉涓€鑷达紙鍝€曞樊 1锛夛紝鍏夋爣鍜岄噸缁樺唴瀹瑰氨鐢昏繘閿欒鍗曞厓鏍硷紱閫忔槑 canvas + 鑳屾櫙鍥句笅锛岀敾閿欎綅鐨勫瓧绗︽诞鍦ㄨ儗鏅殑閿欒浣嶇疆 鈫?瑙嗚"瀛楃璺戝埌鑳屾櫙鏃併€佹妸鑳屾櫙鎸ゅ亸"銆傞殢鏈烘槸鍥犱负鍙湁杈撳叆瑙︾鍒楄竟鐣岋紙杩戣灏?/ 鍥炲嵎锛夋墠鏄剧幇銆俢ols 涓嶄竴鑷寸殑鍓╀綑鏍瑰洜 = **瀛椾綋寮傛鍔犺浇绔炴€?*锛歂erd Font 鏂囦欢娌″姞杞藉畬鏃?`fit()` 鐢?fallback 瀛椾綋閲?cellWidth 鈫?cols 鍋忥紱瀛椾綋灏辩华鍚?cellWidth 鍙樹絾 cols 娌￠噸绠椼€備笖浠ｇ爜閲?`fontFamily` 鍙樺寲鏃跺彧鏀?`term.options.fontFamily`銆?*娌℃湁閲嶆柊 fit**鈥斺€攛term 鏀瑰瓧浣撻噸鏂伴噺 cellWidth 鍗翠笉閲嶇畻 cols锛宍cols 脳 cellWidth` 闈欓粯婕傜瀹瑰櫒瀹藉害銆?
- **淇锛?* `TerminalPanel.tsx` 鐨?fontFamily effect锛氭敼瀛椾綋鍚庯紙鍚?mount 鍒濆瑙﹀彂锛夌瓑 `document.fonts.ready` 鍐?`fit()` + `resizeTo()`锛屾妸瀛椾綋灏辩华鍚庣殑姝ｇ‘ cols 鎺ㄧ粰 PTY銆傝鐩栧垵濮嬪瓧浣撶珵鎬?+ 鍚庣画瀛椾綋鍒囨崲涓や釜鍦烘櫙銆傛部鐢?ResizeObserver 鐨?`clientWidth<80` 灏忓昂瀵镐繚鎶ゃ€?
- **鐮旂┒渚濇嵁锛?* xterm.js 绀惧尯鍚岀被闂缁忛獙鈥斺€擺Issue #2252](https://github.com/xtermjs/xterm.js/issues/2252)锛圵ebGL 閫忔槑锛夈€乕#1901](https://github.com/xtermjs/xterm.js/issues/1901)锛坆uffer line 鍏夋爣璺筹級銆乕#3287](https://github.com/xtermjs/xterm.js/issues/3287)锛坓lyph 瀹氫綅锛? 閫氱敤寤鸿銆宖it() 鍓嶇瓑 `document.fonts.ready`銆佹敼 fontFamily 鍚庡繀椤婚噸鏂?fit銆嶃€?
- **淇敼鏄庣粏锛?* `src/components/TerminalPanel.tsx` 鈥斺€?fontFamily effect 鎵╁睍锛歚document.fonts.ready.then(refit)`锛坈atch 鍏滃簳锛夛紝refit 鍐呭惈灏忓昂瀵镐繚鎶?+ fit + resizeTo銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆傚緟 `cargo tauri dev` 瀹炴祴锛氭湰鍦?pwsh + 鑳屾櫙鍥撅紝杈撳叆闀垮懡浠や笉鍐嶉殢鏈洪敊浣?/ 鎸よ儗鏅€?
- **閬楃暀 / 浜屽垎璇婃柇锛?* 鑻ュ瓧浣?fit 淇鍚?*浠嶆湁**闂锛岃涓存椂鍦ㄨ缃噷**鍏虫帀鑳屾櫙鍥?*娴嬭瘯鈥斺€斺憼 鍏宠儗鏅浘鍚庢甯?鈫?纭鏄?WebGL 閫忔槑鍚堟垚璺緞鐙珛闂锛圛ssue #2252 绫伙級锛屼笅涓€姝ヨ€冭檻 canvas 鍥為€€锛堜絾浼氬甫鍥炴畫褰憋級鎴栧崌绾?xterm / 璋冮€忔槑绛栫暐锛涒憽 鍏宠儗鏅浘浠嶆湁 鈫?绾?cols 闂锛岀户缁帓鏌?initial 80脳24 璺冲彉 / ResizeObserver 鐬椂灏哄銆?
- **鍏宠仈锛?* 闃舵 18锛圵ebGL 閫忔槑閲嶇粯锛? 闃舵 19锛坧adding 淇?cols 鍑犱綍锛? 闃舵 20锛堝瓧浣?fit 淇?cols 鏃跺簭锛変笁灞傚彔鍔犳不鐞?杈撳叆涔辫烦 / 鎸よ儗鏅?鈥斺€斿悓涓€鐥囩姸鐨勫灞傛牴鍥犮€?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 20 complete锛堝瓧浣撳姞杞界珵鎬佽嚧 cols 婕傜Щ淇锛宼sc 缁匡紝寰?dev 瀹炴祴锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴鏈湴 pwsh + 鑳屾櫙鍥捐緭鍏ヤ笉鍐嶉殢鏈洪敊浣嶏紱鑻ヤ粛鏈夛紝鎸?鍏宠儗鏅浘浜屽垎"鍒ゆ柇鏄惁 WebGL 璺緞鐙珛闂 |
| 鐩爣鏄粈涔堬紵 | 淇帀闃舵 19 padding 涔嬪悗浠嶆畫鐣欑殑"瀛楃鎸よ儗鏅?鈥斺€旇繖娆℃牴鍥犳槸瀛椾綋寮傛鍔犺浇璁?cols 婕傜Щ銆丳SReadLine 鎸夐敊璇?cols 閲嶇粯 |
| 鎴戝鍒颁簡浠€涔堬紵 | xterm 鏀?fontFamily 閲嶆柊閲?cellWidth 浣?*涓嶉噸绠?cols**锛屽繀椤绘墜鍔?fit锛沠it() 鍓嶅簲绛?`document.fonts.ready`锛汸SReadLine 姣忛敭缁濆瀹氫綅閲嶇粯銆佸 cols 鍋忓樊闆跺蹇嶏紱"杈撳叆涔辫烦"鏄灞傛牴鍥狅紙閫忔槑娈嬪奖 / padding / 瀛椾綋鏃跺簭锛夛紝闇€閫愬眰鍓?+ 鐢?AskUserQuestion 閿佺幆澧冨啀鏀癸紝閬垮厤鐩茬洰淇亸 |
| 鎴戝仛浜嗕粈涔堬紵 | AskUserQuestion 閿佸畾锛堣儗鏅浘 + pwsh + 闅忔満锛? 鐮旂┒ xterm 绀惧尯缁忛獙 + TerminalPanel fontFamily effect 鍔?document.fonts.ready 鈫?fit + resize + tsc 楠岃瘉 |

### 闃舵 21锛氭湰鍦扮粓绔緭鍏ュ洖鍗烽敊浣嶏紙涓夛級鈥斺€?PTY 鍒濆 cols 鍚屾鏃舵満锛堝喅瀹氭€ф牴鍥狅級锛?026-06-19锛?
- **鐜拌薄锛堢敤鎴风粰鍑哄畬缇庡鐜扮偣锛夛細** 闃舵 20 瀛椾綋 fit 淇鍚庯紝鐢ㄦ埛閿佸畾**鐧惧垎鐧惧鐜?*鏉′欢鈥斺€旇緭鍏ュ唴瀹硅秴杩囨湰琛岋紙瑙﹀彂鍥炲嵎锛夊繀鐜?鑳屾櫙宸︾Щ"锛?*鏃犺儗鏅浘涔熷鐜?*锛堟帓闄?WebGL 閫忔槑璺緞锛夛紱杈撳叆瀹屾垚鎹㈣鍚?*鎭㈠姝ｅ父**銆?
- **鏍瑰洜锛堟帢閲?juejin/7476761846411870258 100% 鍖归厤楠岃瘉锛夛細** 缁忓吀 xterm鈫擯TY cols 涓嶅悓姝ャ€傛湰鍦?PTY 浠?**80脳24 鍚姩**锛坢ain.rs `local_connect` 鍐欐 cols=80锛夛紝pwsh 鐨?PSReadLine 缂撳瓨浜?80锛涘墠绔?xterm 缁?fit 寰楀埌鐪熷疄 cols锛堝 120锛夈€傚悓姝ラ摼涓ゅ鏃舵満閮藉お璧讹細鈶?mount 100ms resize 鎾炲湪 PSReadLine 鍒濆鍖?*涔嬪墠** 鈫?琚涪寮冿紱鈶?firstOutput 鐨?resize 鏄?`setTimeout(..., 0)`锛孭SReadLine 鍒氱敾瀹?prompt銆乺esize listener 杩樻病鎺ョ 鈫?鍐嶆涓㈠純銆傜粨鏋滃悗绔?cols 鍗″湪 80銆佸墠绔?120锛岃緭鍏ヨ繃绗?80 鍒楀悗绔崲琛屽墠绔笉鎹㈣ 鈫?PSReadLine 閲嶇粯缂栬緫琛岃繘閿欏崟鍏冩牸 = "鑳屾櫙宸︾Щ"锛汦nter 鍚庢柊 prompt 鍗曡鏁呮仮澶嶃€傚洖鍗疯竟鐣?100% 澶嶇幇瀹屽叏鍚诲悎銆?
- **淇锛?* `TerminalPanel.tsx` firstOutput 鎶婂崟娆?`setTimeout(0)` resize 鏀逛负**绔嬪嵆 + 250ms + 600ms 澶氭寤惰繜鍚屾**锛坒it + resizeTo锛夛紝瑕嗙洊涓嶅悓 shell 鍐峰惎鍔ㄩ€熷害涓?PSReadLine/ConPTY 灏辩华绐楀彛锛堟帢閲戦獙璇?200ms 鏈夋晥锛屽娆℃洿绋筹級銆傛柊澧?`firstSyncTimersRef` + [sessionId] cleanup 娓呯悊 timers銆?
- **鐮旂┒渚濇嵁锛?* [鎺橀噾锛歺term.js 杈撳叆瀛楃鎹㈣瑕嗙洊鎺掓煡](https://juejin.cn/post/7476761846411870258)锛?COLUMNS=80 + resize 鍚庢甯?+ onResize/200ms 闃叉姈 + 棣栧抚 output 瑙﹀彂 resize锛夛紱[Issue #3342](https://github.com/xtermjs/xterm.js/issues/3342) nerd font 瀹藉瓧绗﹁鍓紙鐩稿叧浣嗛潪鏈涓诲洜锛夈€?
- **淇敼鏄庣粏锛?* `src/components/TerminalPanel.tsx` 鈥斺€?firstOutput 鍚屾鏀逛负澶氭寤惰繜锛?/250/600ms锛夛紱鏂板 firstSyncTimersRef ref锛沜leanup 娓呯悊銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆傚緟 `cargo tauri dev` 瀹炴祴锛氭湰鍦?pwsh 杈撳叆瓒呰繃 80 鍒椾笉鍐嶅洖鍗烽敊浣?/ 鑳屾櫙宸︾Щ銆?
- **閬楃暀 / 璇婃柇鍛戒护锛?* 鑻ヤ粛鏈夐棶棰橈紝鍦ㄥ嚭闂鐨勬湰鍦扮粓绔窇 `$Host.UI.RawUI.WindowSize.Width`鈥斺€斺憼 浠嶆槸 80 鈫?resize 娌￠€佽揪 ConPTY锛堝崌绾?portable-pty 0.8 鎴栨煡 ConPTY resize 鏃跺簭锛夛紱鈶?宸叉槸鐪熷疄鍊间絾浠嶉敊浣?鈫?杞悜 xterm 娓叉煋灞傦紙WebGL/canvas 瀵规瘮銆乶erd font 瀹藉瓧绗?#3342銆乧learTextureAtlas锛夈€?
- **鍏宠仈锛?* 闃舵 18锛圵ebGL 閫忔槑娈嬪奖锛? 19锛坧adding 淇?cols 鍑犱綍锛? 20锛堝瓧浣?fit 淇?cols 鏃跺簭锛? 21锛圥TY 鍒濆鍚屾閫佽揪锛夊洓灞傚彔鍔犮€?*鍓嶄笁娆′慨鐨勬槸"鍓嶇 cols 绠楀緱瀵逛笉瀵?锛屾湰娆′慨鐨勬槸"鍓嶇绠楀浜嗕箣鍚庢湁娌℃湁鎶婃纭?cols 閫佽揪鍚庣 PTY"**鈥斺€旇繖鏄?SSH/PTY 缁堢缁忛獙閲屾渶缁忓吀銆佹渶鏄撴紡鐨勪竴鐜€?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 21 complete锛圥TY 鍒濆 cols 鍚屾鏃舵満淇锛宼sc 缁匡紝寰?dev 瀹炴祴锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴鏈湴 pwsh锛氳緭鍏ヨ秴杩?80 鍒椾笉鍐嶅洖鍗烽敊浣嶏紱鑻ヤ粛鏈夎窇 `$Host.UI.RawUI.WindowSize.Width` 璇婃柇 cols 鏄惁閫佽揪 |
| 鐩爣鏄粈涔堬紵 | 褰诲簳瑙ｅ喅"杈撳叆鍥炲嵎蹇呯幇鑳屾櫙宸︾Щ銆佹崲琛屾仮澶?鈥斺€旀牴鍥犳槸 PTY 浠?80 鍚姩銆佸垵濮?resize 鏃舵満澶棭琚涪寮冿紝鍚庣鍗?80 |
| 鎴戝鍒颁簡浠€涔堬紵 | xterm鈫擯TY cols 鍚屾鏄粓绔渶缁忓吀 bug锛歅TY 鍚姩 cols锛?0锛夆墵 鍓嶇鐪熷疄 cols锛屽繀椤诲湪 shell 灏辩华鍚庯紙棣栧抚 output + 鍏呭垎寤惰繜锛夋妸鐪熷疄 cols resize 缁欏悗绔紝鏃舵満瑕佺粰瓒筹紙PSReadLine 鍒濆鍖栨參锛?ms 涓嶅锛岄渶 200ms+锛屽娆℃洿绋筹級锛涙帢閲戝疄鎴樻枃绔犳瘮娉涙悳鏇寸簿鍑嗭紱"鎹㈣鍚庢仮澶?+ 鍥炲嵎杈圭晫 100% 澶嶇幇"鏄?cols 涓嶅悓姝ョ殑鎸囩汗 |
| 鎴戝仛浜嗕粈涔堬紵 | 鎺橀噾鏂囩珷閿佸畾 cols 涓嶅悓姝ユ牴鍥?+ firstOutput 鍗曟 0ms 鈫?澶氭寤惰繜锛?/250/600ms锛夊悓姝?+ firstSyncTimersRef cleanup + tsc 楠岃瘉 |

### 闃舵 22锛歝laude TUI 杈撳叆宸︾Щ锛堝喅瀹氭€ф牴鍥狅級鈥斺€?init_command 鍚姩鏃舵満锛?026-06-19锛?
- **鐜拌薄锛?* 闃舵 21 cols 鍚屾淇鍚庯紝鐢ㄦ埛缁欏嚭**鍐冲畾鎬х嚎绱?*鈥斺€斿乏绉?*鍙湁 claude锛圕laude Code CLI锛夐噷鎵嶆湁**锛屾櫘閫氬懡浠わ紙ls/dir锛夊畬鍏ㄦ甯革紱涓?claude 鏄厤缃?*銆屽惎鍔ㄥ懡浠ゃ€嶈嚜鍔ㄥ惎鍔?*鐨勩€?
- **鎺ㄧ悊锛?* "鍙湁 claude" 鎺掗櫎浜?cols 鍚屾瀵?PSReadLine 鐨勯棶棰樷€斺€旀櫘閫氬懡浠ゆ甯?= PowerShell 琛岀紪杈戞嬁鍒扮殑鍒楁暟鏄鐨勶紝鍗抽樁娈?19-21 鐨?cols 鍚屾**纭疄鐢熸晥**浜嗐€傞棶棰樿仛鐒﹀埌 claude 杩欎釜 TUI 搴旂敤鏈韩銆?
- **鏍瑰洜锛?* claude 鏄氦浜掑紡 TUI锛屽惎鍔ㄦ椂璇诲彇缁堢鍒楁暟甯冨眬鐣岄潰锛堣緭鍏ユ瀹藉害锛夛紝涓?*鍚姩鍚庝笉璺熼殢 resize**銆傚畠閫氳繃銆屽惎鍔ㄥ懡浠ゃ€嶏紙init_command锛夎嚜鍔ㄥ惎鍔紝鑰?init_command 鍦?PTY **鍒氫互 80脳24 鍚姩鏃跺氨琚珛鍗冲啓鍏?stdin**锛坙ocal.rs writer task 鍚姩鍗冲彂锛夆啋 claude 鍚姩璇诲埌 **80 鍒?* 鈫?TUI 鎸?80 甯冨眬 鈫?涔嬪悗鍓嶇 resize 鍒扮湡瀹炲搴︼紙濡?120锛変絾 claude 宸茬紦瀛?80 涓嶆洿鏂?鈫?杈撳叆杩囩 80 瀛楃鏃?claude 鎸夊叾璁ょ煡鐨?80 鍒楅噸缁樿緭鍏ユ锛屽湪瀹為檯涓?120 瀹界殑缁堢閲岄敊浣嶃€佸乏绉伙紱Enter 鍚庢柊 prompt 鍗曡鏁呮仮澶嶃€傛櫘閫氬懡浠や笉缂撳瓨鍒楁暟锛堟瘡娆℃寜褰撳墠瀹藉害杈撳嚭锛夋晠瀹屽叏姝ｅ父銆?*瀹岀編瑙ｉ噴"鍙湁 claude + 鍥炲嵎杈圭晫 100% 澶嶇幇 + 鎹㈣鎭㈠"**銆?
- **淇锛?* `src-tauri/src/local.rs` writer task锛氭妸 init_command 浠庛€屽惎鍔ㄦ椂绔嬪嵆鍙戙€嶆敼涓恒€?*绗竴娆?resize 鍒拌揪鍚庡啀鍙?*銆嶃€傜涓€娆?resize = 鍓嶇宸?fit 鍑虹湡瀹炲昂瀵稿苟鍚屾缁?PTY 鐨勪俊鍙凤紝姝ゆ椂鍐嶅彂 init_command锛宑laude 鍚姩璇诲埌鐨勫氨鏄湡瀹炲垪鏁帮紙鑰岄潪 80锛夈€倁tf8_prelude 浠嶇珛鍗冲彂锛坰hell 缂栫爜璁剧疆闇€鍦ㄥ惎鍔ㄦ棭鏈燂級锛沗pending_init` 鐢?`take()` 淇濊瘉鍙彂涓€娆°€?
- **鍓綔鐢紙棰勬湡锛夛細** pwsh 鎻愮ず绗︿細**鍏堢煭鏆傚嚭鐜?*锛坕nit_command 寤惰繜鍒扮涓€娆?resize ~100ms 鍚庢墠鍙戯級锛岄殢鍚?claude 鍚姩銆傚彲鎺ュ彈銆傞潪 TUI 鐨?init_command锛堝 cd锛夊欢杩熸墽琛屼害鏃犲銆?
- **淇敼鏄庣粏锛?* `src-tauri/src/local.rs` 鈥斺€?绉婚櫎 init_command 鍚姩鍗冲彂鍧楋紝鏀逛负 `pending_init: Option<String>`锛汻esize 鍒嗘敮 `master.resize` 鍚?`if let Some(init) = pending_init.take() { write + \r + flush }`銆?
- **楠岃瘉锛?* `cargo check` PASS锛?*棣栨**鈥斺€擱ust 宸ュ叿閾剧幇宸插湪缂栬緫鐜鍙敤锛屼箣鍓嶅悇闃舵 Rust 渚ф湭缂栬瘧杩囷級銆傚緟 `cargo tauri dev` 瀹炴祴锛歝laude 杈撳叆瓒呰繃鏈涓嶅啀宸︾Щ銆?
- **鍏宠仈锛?* 闃舵 18-21 淇殑鏄?cols 鐨?*姝ｇ‘鎬?+ 鍚屾**锛堝墠绔畻瀵广€佸悓姝ョ粰 PTY锛夛紝浣?claude TUI 鍦?cols 鍚屾**涔嬪墠**灏卞惎鍔ㄥ苟缂撳瓨浜?80銆傛湰娆★紙22锛変慨鐨勬槸銆?*璁?claude 鍦?cols 鍚屾涔嬪悗鍐嶅惎鍔?*銆嶁€斺€斿悓涓€鏉?宸︾Щ"鐥囩姸锛屼簲灞傛牴鍥狅紙閫忔槑娈嬪奖 / padding / 瀛椾綋鏃跺簭 / PTY 鍚屾 / **TUI 鍚姩鏃舵満**锛夐€愬眰鍓ュ紑銆?
- **鏁欒锛?* "鍙湁鏌愪釜 TUI 搴旂敤鎵嶆湁"鏄叧閿俊鍙封€斺€擳UI 缂撳瓨缁堢灏哄涓斾笉璺熼殢 resize锛屼笌鏅€?shell 琛屼负涓嶅悓锛沬nit_command 绫荤殑鑷姩鍚姩鍛戒护搴斿湪缁堢灏哄纭畾鍚庡啀鍙戯紝鍚﹀垯 TUI 鎷垮埌鐨勬槸 PTY 鍚姩榛樿鍊硷紙80锛夛紱杩炵画鐩叉敼鏃犳晥鏃惰鍥炲埌"浠€涔堝満鏅墠鏈?/ 娌℃湁璇ョ幇璞?鐨勫姣旀潵缂╁皬鑼冨洿銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 22 complete锛坕nit_command 寤惰繜鍒?cols 鍚屾鍚庯紝cargo check 缁匡紝寰?dev 瀹炴祴锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 cargo tauri dev 瀹炴祴锛歝laude 杈撳叆瓒呴暱涓嶅啀宸︾Щ锛涚暀鎰?pwsh prompt 鍏堢煭鏆傞棯鐜板啀杩?claude锛堥鏈熷壇浣滅敤锛?|
| 鐩爣鏄粈涔堬紵 | 淇帀"鍙湁 claude 鎵嶆湁鐨勮緭鍏ュ乏绉?鈥斺€旀牴鍥犳槸 claude 浣滀负鍚姩鍛戒护鍦?PTY 80脳24 鏃跺氨鍚姩銆佺紦瀛?80 鍒椾笉璺熼殢 resize |
| 鎴戝鍒颁簡浠€涔堬紵 | "鍙湁鏌?TUI 鎵嶆湁" = TUI 缂撳瓨缁堢灏哄涓嶈窡闅?resize锛坴s 鏅€?shell 瀹炴椂璇伙級锛宨nit_command 蹇呴』鍦ㄥ昂瀵哥‘瀹氬悗鍙戯紱杩炵画鐩叉敼鏃犳晥鏃惰鍥炲埌"浠€涔堝満鏅墠鏈?娌℃湁璇ョ幇璞?鐨勫姣旂缉灏忚寖鍥达紱Rust 宸ュ叿閾剧幇宸插彲鐢紝cargo check 鑳介獙璇佸悗绔敼鍔?|
| 鎴戝仛浜嗕粈涔堬紵 | AskUserQuestion 纭 claude = 鍚姩鍛戒护鑷姩鍚姩 + local.rs init_command 鍚姩鍗冲彂 鈫?绗竴娆?resize 鍚庡彂锛坧ending_init + Resize 鍒嗘敮 take锛? cargo check PASS |

### 闃舵 23锛氫腑鏂?IME 杈撳叆宸︾Щ 鈥斺€?ConPTY 涓婃父 bug锛堝凡鐭ラ檺鍒讹紝搴旂敤灞傛棤娉曟牴娌伙級锛?026-06-19锛?
- **鐜拌薄锛堟洿绮剧‘绾跨储锛夛細** 闃舵 22 鍚庯紝鐢ㄦ埛杩涗竴姝ュ畾浣嶁€斺€斿乏绉?*鍙湪涓枃 IME 杈撳叆鏃跺彂鐢?*锛屽瓧姣?鑻辨枃杈撳叆瀹屽叏姝ｅ父锛汸owerShell 鍜?claude 閮芥湁銆?
- **鎺掓煡锛堝懡涓搧璇侊級锛?* 鎼滅储鍛戒腑 [VSCode #255285 "Terminal viewport shifts left with Chinese IME"](https://github.com/microsoft/vscode/issues/255285)锛岀幇璞?瑙﹀彂/鏍瑰洜**閫愬瓧鍖归厤**鈥斺€?IME composition string 鎵撳埌绗?4 涓瓧绗︼紝鏁翠釜缁堢瑙嗗彛姘村钩宸︾Щ锛涙寜绌烘牸瀹屾垚杈撳叆鍚庣珛鍗虫仮澶?銆?
- **鏍瑰洜锛?* **ConPTY 瀵逛腑鏂?IME composition string 鐨勫搴﹁绠楅敊璇紙miscalculation锛?*锛岃Е鍙戜笉蹇呰鐨勮鍙ｅ乏绉汇€傝繖鏄?**ConPTY 鐨勪笂娓?bug**銆傝Е鍙戞潯浠讹細浼?姣忛敭閲嶇粯杈撳叆琛?鐨勭▼搴忥紙PSReadLine銆乧laude/ink銆乬emini-cli锛夛紱vim / node / python REPL 涓嶈Е鍙戯紙瀹冧滑涓嶅疄鏃堕噸缁樿緭鍏ヨ锛夆€斺€?瀹岀編瑙ｉ噴"瀛楁瘝姝ｅ父锛堝崟瀹斤紝閲嶇粯鏃犳涔夛級+ 涓枃宸︾Щ锛堝弻瀹?composition 瀹藉害绠楅敊锛? PowerShell 鍜?claude 閮芥湁"銆?
- **VSCode 纭鐨?workaround锛?* 绂佺敤 ConPTY銆佹敼鐢?**winpty** 鍚庣锛圴S Code `terminal.integrated.windowsEnableConpty: false`锛夈€?
- **鎴戜滑鐨勫洶澧冿紙鍏抽敭锛夛細** 鎴戜滑鐢ㄧ殑 **portable-pty 0.8.1 宸茬Щ闄?winpty 鏀寔**锛坄src/win/` 鍙湁 `conpty.rs`锛屾棤 `winpty.rs`锛沗NativePtySystem = ConPtySystem`锛夈€傛墍浠?**winpty workaround 瀵规垜浠笉鍙**銆傞檷绾?portable-pty 鎴栨崲 winpty-rs 浠ｄ环澶э紝涓?**winpty 瀵逛腑鏂?UTF-8 涓嶅弸濂?*锛堣蛋 Windows console codepage锛屽彲鑳藉紩鍏ヤ腑鏂囦贡鐮侊紝姣斿乏绉绘洿绯燂級銆?
- **缁撹锛?* 杩欐槸 **ConPTY 涓婃父 bug锛屽簲鐢ㄥ眰鏃犳硶瀹岀編淇**銆傞渶绛夊井杞慨澶嶏紙VSCode #255285 open锛?025-07 鎶ワ紝鐩墠鏈慨锛涚敤鎴?Windows 11 26200 浠嶅瓨鍦級銆?
- **鍓嶅嚑杞慨澶嶇殑浠峰€硷紙閲嶈锛夛細** 闃舵 18-22 鐨勪慨澶嶏紙WebGL 閫忔槑娈嬪奖 / padding cols 鍑犱綍 / 瀛椾綋 cols 鏃跺簭 / PTY 鍚屾鏃舵満 / TUI 鍚姩鏃舵満锛?*閮芥槸瀵圭殑銆佹湁浠峰€肩殑**鈥斺€?瀛楁瘝杈撳叆瀹屽叏姝ｅ父鎹㈣"灏辨槸璇佹槑锛坈ols 鍚屾姝ｇ‘銆丳SReadLine 鎷垮埌鐪熷疄鍒楁暟锛夈€?*涓枃 IME 鏄敮涓€鍓╀笅鐨勩€佺嫭绔嬬殑 ConPTY 涓婃父闄愬埗**锛屼笌鍓嶅嚑杞棤鍏炽€?
- **瀹炵敤缂撹В锛堝鐢ㄦ埛锛夛細** 鈶?杈撳叆娉曠‘璁わ紙绌烘牸/鍥炶溅锛夊悗宸︾Щ**绔嬪嵆鎭㈠**锛屽奖鍝嶄粎 composition 杈撳叆杩囩▼涓紱鈶?閬垮厤涓€娆℃墦瓒呴暱 composition锛屽垎娈电‘璁ゅ彲鍑忓皯瑙﹀彂锛涒憿 绛?Windows 鏇存柊淇 ConPTY銆?
- **淇敼锛?* 鏈鏃犱唬鐮佷慨鏀癸紙搴旂敤灞傛棤娉曚慨澶?ConPTY bug锛夛紱浠呯爺绌剁‘璁?+ 鏂囨。璁板綍锛岄伩鍏嶆湭鏉ラ噸澶嶆帓鏌ャ€?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 23锛氱‘璁や腑鏂?IME 宸︾Щ鏄?ConPTY 涓婃父 bug锛圴SCode #255285 閾佽瘉锛夛紝搴旂敤灞傛棤娉曟牴娌伙紝璁板綍涓哄凡鐭ラ檺鍒?|
| 鎴戣鍘诲摢閲岋紵 | 鍛婄煡鐢ㄦ埛鏍瑰洜 + 鍥板锛坧ortable-pty 0.8.1 鏃?winpty锛? 瀹炵敤缂撹В锛涚瓑寰蒋淇 ConPTY |
| 鐩爣鏄粈涔堬紵 | 缁?涓枃 IME 宸︾Щ"涓€涓噯纭€佹湁鎹殑缁撹锛岄伩鍏嶅弽澶嶇洸鏀?|
| 鎴戝鍒颁簡浠€涔堬紵 | "鍙湪鏌愮杈撳叆锛堜腑鏂?IME / 鍙屽锛夋墠鍑?= 涓婃父 CJK/IME 澶勭悊 bug 鐨勪俊鍙凤紱ConPTY 瀵?IME composition 瀹藉害璁＄畻鏈夊凡鐭?bug锛圴SCode 涔熶腑鎷涳級锛沺ortable-pty 0.8.x 宸茬Щ闄?winpty锛堝彧 ConPTY锛夛紝winpty workaround 瀵规垜浠笉鍙锛涢亣鍒颁笂娓?bug 瑕佹煡 VSCode/microsoft-terminal 鍚屾 issue锛屾瘮娉涙悳绮惧噯 |
| 鎴戝仛浜嗕粈涔堬紵 | WebSearch 鍛戒腑 VSCode #255285锛堥搧璇侊級+ webReader 璇诲叏鏂?+ 纭 portable-pty 0.8.1 鏃?winpty锛坰rc/win/ 鍙湁 conpty.rs锛? 璁板綍宸茬煡闄愬埗 |

### 闃舵 24锛氬洖閫€闃舵 22锛坕nit_command 寤惰繜锛夆€斺€?鍓綔鐢ㄤ笉鍊硷紙2026-06-19锛?
- **鍐崇瓥锛?* 鐢ㄦ埛鎷嶆澘鍥為€€闃舵 22锛坙ocal.rs init_command 寤惰繜鍒伴娆?resize锛夈€?
- **鐞嗙敱锛?* 闃舵 22 淇殑"claude TUI 浠?80 cols 鍚姩"鏄湡瀹炰絾鐙珛鐨勯棶棰橈紝涓?*娌′慨澶?*鐢ㄦ埛鎶ュ憡鐨勫乏绉伙紙閭ｅ叾瀹炴槸闃舵 23 纭鐨?ConPTY 涓枃 IME 涓婃父 bug锛夛紱鑰屽畠鐨勫壇浣滅敤鈥斺€斿紑 tab 鏃?PowerShell 鎻愮ず绗?*鍏堢煭鏆傞棯鐜板啀杩?claude**鈥斺€斿彲鎰熺煡銆佷笉鍊笺€傛潈琛″悗鍥為€€銆?
- **鏀瑰姩锛?* `src-tauri/src/local.rs` writer task 鈥斺€?init_command 浠?绗竴娆?resize 鍚庡彂"鏀瑰洖"鍚姩鏃剁珛鍗冲彂"锛堟仮澶嶉樁娈?21 瀹屾垚鏃剁殑鐘舵€侊級锛汻esize 鍒嗘敮绉婚櫎 pending_init 閫昏緫锛涗繚鐣欎竴鏉℃敞閲婅鏄庝负浠€涔堢珛鍗冲彂 + 鏇捐瘯杩囧欢杩熶絾鍥為€€锛岄伩鍏嶆湭鏉ラ噸澶嶅皾璇曘€?
- **淇濈暀涓嶅姩锛?* 闃舵 19锛坧adding cols 鍑犱綍锛夈€?0锛堝瓧浣?fit锛夈€?1锛坒irstOutput 澶氭寤惰繜 resize锛夈€乄ebGL 鏉′欢鍔犺浇鈥斺€旇繖浜涙槸 cols 姝ｇ‘鎬?/ 鍚屾 / 娓叉煋鐨勫噣鏀剁泭锛屼笌闃舵 22 鏃犲叧锛?瀛楁瘝姝ｅ父鎹㈣"灏辨槸瀹冧滑鐨勬垚鏋溿€?
- **楠岃瘉锛?* `cargo check` PASS锛?.50s 澧為噺锛夈€?
- **缁撹锛?* 鏈€缁堜唬鐮?= 闃舵 19/20/21 + WebGL 鏉′欢鍔犺浇**淇濈暀**锛岄樁娈?22 **鍥為€€**銆備腑鏂?IME 宸︾Щ = ConPTY 涓婃父 bug锛堥樁娈?23锛夛紝搴旂敤灞備笉淇€?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 24锛氬洖閫€闃舵 22锛坕nit_command 寤惰繜锛夛紝cargo check 缁?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 楠岃瘉锛氬紑 claude tab 涓嶅啀鏈?prompt 闂幇锛堝惎鍔ㄥ懡浠ょ珛鍗虫墽琛岋級锛涗腑鏂?IME 宸︾Щ鎸夊凡鐭ラ檺鍒讹紙闃舵 23锛夋帴鍙?|
| 鐩爣鏄粈涔堬紵 | 绉婚櫎闃舵 22 鐨勫壇浣滅敤锛坧rompt 闂幇锛夛紝淇濈暀鍏朵綑鍑€鏀剁泭淇 |
| 鎴戝鍒颁簡浠€涔堬紵 | 淇湡闂涔熻鏉冭　鍙劅鐭ュ壇浣滅敤锛涘綋鏌愪釜淇鏈€缁堟病鍛戒腑鐢ㄦ埛鐪熷疄闂锛堝疄涓轰笂娓?bug锛夋椂锛屽畠鐨勫壇浣滅敤灏变笉鍊煎緱锛屽簲鍥為€€锛涘洖閫€瑕佺暀娉ㄩ噴璇存槑鏇惧皾璇?+ 涓轰粈涔堟斁寮冿紝闃查噸澶?|
| 鎴戝仛浜嗕粈涔堬紵 | local.rs init_command 寤惰繜 鈫?鍚姩鍗冲彂锛堝洖閫€闃舵 22锛? 淇濈暀闃查噸澶嶆敞閲?+ cargo check PASS + 鏂囨。鍚屾锛坧rogress 闃舵 24 / README 绉婚櫎闃舵 22 鏉＄洰锛?|

### 闃舵 25锛氶泦鎴?AI 鍔╂墜锛堝鎻愪緵鍟嗚亰澶?+ 鍛戒护鐢熸垚/璇婃柇/瑙ｉ噴 + 涓诲姩宸℃锛夛紙2026-06-19锛?
- **鐩爣锛?* 鍦ㄧ粓绔伐鍏峰唴闆嗘垚 AI锛岃鐩栧懡浠ょ敓鎴愩€佽緭鍑鸿瘖鏂€佸懡浠よВ閲娿€佹湇鍔″櫒宸℃銆傚鎻愪緵鍟嗭紙Claude/OpenAI/Ollama锛夈€佸叏灞€鍙充晶鑱婂ぉ鏍忋€丄PI key 澶嶇敤 vault銆丄I 璋冪敤鍦?Rust 鍚庣娴佸紡杈撳嚭銆?
- **鏂规**锛坧lan `twinkly-mapping-floyd.md`锛屽凡鎵瑰噯锛夛細4 Phase銆?
- **Phase 1 鍚庣鏍稿績锛?* `Cargo.toml` +reqwest(rustls+json+stream)+futures-util锛沗db.rs` 鍔?`ai_settings`(鍗曡,_enc key)+`ai_conversations` 琛紱鏂版ā鍧?`ai.rs`锛圥rovider enum[Claude/OpenAi/Ollama] + 鍚勮嚜 endpoint/auth/body/SSE路NDJSON 瑙ｆ瀽 + `chat_stream` 娴佸紡 emit ai_token/done/error + vault 鍙?key[decrypt_with_key] + Linux/Windows 鍙宸℃鑴氭湰 + `inspect_health_ssh`[澶嶇敤 ssh::exec_once]/`local`[std::process]锛夛紱`main.rs` +`mod ai` +5 鍛戒护(ai_chat/ai_inspect_health_ssh/ai_inspect_health_local/get_ai_settings/save_ai_settings锛屽潎 require_dek)銆?
- **Phase 2 鍓嶇闈㈡澘锛?* `package.json` +react-markdown+remark-gfm锛沗api.ts` +aiChat/onAiToken/onAiDone/onAiError + AiSettings/ChatMessage/AiContext 绫诲瀷锛沗useAiConfig` hook(璧?IPC)锛涙柊缁勪欢 `AiPanel.tsx`(鍏ㄥ眬 docked 鍙虫爮, 娑堟伅鍒楄〃, 娴佸紡娓叉煋, react-markdown, 浠ｇ爜鍧楀鍒? 鎷栨嫿璋冨)锛沗App.tsx` +showAiPanel/aiPanelWidth state + 鎸傝浇 + activeTab 涓婁笅鏂囷紱`Sidebar` +馃 鎸夐挳锛沗SettingsPanel` +馃 AI 鍔╂墜 Section(provider select/key/model/baseUrl/temperature)銆?
- **Phase 3 缁堢闆嗘垚锛?* `TerminalPanel` +onTerminalReady/onTerminalGone props(鏆撮湶 xterm 瀹炰緥)锛沗App` 缁存姢 Map<sessionId,Terminal> registry + getTerminal锛沗AiPanel` 閲囬泦閫夊尯[getSelection]/鏈€杩?40 琛孾buffer.active.getLine.translateToString]浣滀笂涓嬫枃 + 浠ｇ爜鍧?鎻掑叆缁堢"鎸夐挳[term.paste锛屼笉鑷姩鎵ц] + "闄勫甫閫夊尯"鎸夐挳銆?
- **Phase 4 涓诲姩宸℃锛?* `AiPanel` header +馃攳 宸℃鎸夐挳锛屾寜 connType 璋?aiInspectHealthSsh(sessionId)/aiInspectHealthLocal锛屽鐢ㄦ祦寮忔覆鏌撳仴搴锋姤鍛娿€?
- **瀹夊叏锛?* API key 鍏ㄧ▼ Rust锛坴ault 鍔犲瘑锛屾案涓嶈繘 webview锛夛紱AI 鍛戒护榛樿鍙?鎻掑叆缁堢"涓嶈嚜鍔ㄦ墽琛岋紙鐢ㄦ埛鎵嬪姩 Enter锛夛紱宸℃鑴氭湰涓ユ牸鍙锛坒ree/df/top/uptime 绛夛紝鏃?rm/mv/>锛夛紱AI 璋冪敤缁?vault锛坮equire_dek锛屾湭瑙ｉ攣鍒?AI 涓嶅彲鐢級銆?
- **楠岃瘉锛?* `cargo check` PASS锛圥hase 1锛宺eqwest 棣栨缂栬瘧 23s锛夛紱`npx tsc --noEmit` PASS锛圥hase 2/3/4锛夈€傚緟 `cargo tauri dev` 瀹炴祴锛氳缃～ key 鈫?馃 鑱婂ぉ 鈫?娴佸紡鍥炲锛涢€変腑鎶ラ敊鈫掗檮甯﹂€夊尯鈫掕瘖鏂紱馃攳 宸℃鈫掑仴搴锋姤鍛娿€?
- **淇敼鏂囦欢锛?* 鏂板 `src-tauri/src/ai.rs`銆乣src/components/AiPanel.tsx`銆乣src/hooks/useAiConfig.ts`锛涗慨鏀?`Cargo.toml`銆乣db.rs`銆乣main.rs`銆乣package.json`銆乣api.ts`銆乣App.tsx`銆乣Sidebar.tsx`銆乣SettingsPanel.tsx`銆乣TerminalPanel.tsx`銆?
- **閬楃暀锛?* 鈶?SSE 瑙ｆ瀽涓夊鏍煎紡涓嶅悓锛屾渶鏄撳嚭 bug 澶勶紙寤鸿鍚?provider fixture 鍗曟祴锛夛紱鈶?vault 鏈В閿佹椂 AI 涓嶅彲鐢紙UI 鍦ㄩ敊璇秷鎭彁绀猴級锛涒憿 CommandBar AI 鍏ュ彛鐪佺暐锛圓iPanel 鐨?闄勫甫閫夊尯"宸茶鐩栨牳蹇冿級锛涒懀 宸℃鑴氭湰 Linux/Windows 鍙锛屽彲鎸夐渶鎵╁睍銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 25 complete锛圓I 鍔╂墜 4 Phase锛宑argo check + tsc 缁匡紝寰?dev 瀹炴祴锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 cargo tauri dev 瀹炴祴锛氬～ key鈫掕亰澶┾啋娴佸紡锛涢€夊尯璇婃柇锛涘贰妫€鎶ュ憡 |
| 鐩爣鏄粈涔堬紵 | 闆嗘垚 AI 杈呭姪鍛戒护鐢熸垚/璇婃柇/瑙ｉ噴 + 鏈嶅姟鍣ㄥ贰妫€锛屽鎻愪緵鍟嗭紝vault 淇濇姢 key |
| 鎴戝鍒颁簡浠€涔堬紵 | xterm buffer 璇诲彇(buffer.active.getLine.translateToString + getSelection)锛況eqwest SSE 鐢?bytes_stream + Vec<u8> 鎸夎鍒?閬垮厤 UTF-8 璺?chunk 鎹熷潖)锛汸rovider enum dispatch 姣?async trait 绠€鍗曪紱娴佸紡澶嶇敤 window.emit(鍚?ssh_output)锛汚PI key 澶嶇敤 vault(require_dek + encrypt_with_key)缁熶竴瀹夊叏 |
| 鎴戝仛浜嗕粈涔堬紵 | EnterPlanMode + 2 Explore agent 鎽搁泦鎴愮偣 + AskUserQuestion 閿佸喅绛?澶氭彁渚涘晢/鍏ㄥ眬鏍?vault/棰勮鑴氭湰) + 4 Phase 瀹炵幇(ai.rs/AiPanel/registry/宸℃) + cargo check + tsc 鍏ㄧ豢 + 鏂囨。 |

### 闃舵 26锛欰I 鍔╂墜缃戠粶浠ｇ悊鏀寔锛?026-06-19锛?
- **闇€姹傦細** AI 鍔╂墜闇€鏀寔缃戠粶浠ｇ悊锛堝浗鍐呰闂?Claude/OpenAI 甯搁渶浠ｇ悊锛夈€?
- **鏂规锛?* AI 璁剧疆鍔?`proxy_url` 瀛楁锛況eqwest 鍔?`socks` feature 鏀寔 SOCKS5銆?
- **鏀瑰姩锛?* `Cargo.toml` reqwest +`socks` feature锛沗db.rs` `ai_settings` +`proxy_url` 鍒楋紙+ `column_exists` ALTER 鍏煎鑰佸簱锛夛紱`ai.rs` `LoadedSettings`/`load_settings` 璇?`proxy_url`锛宍run_chat_stream` 鍒涘缓 client 鏃舵寜 `proxy_url` 閰?`reqwest::Proxy::all`锛堟敮鎸?http/https/socks5/socks5h锛寀rl 鍐呭彲鍚?`user:pass@host` 璁よ瘉锛夛紱`main.rs` `get/save_ai_settings` +`proxy_url`锛涘墠绔?`api.ts`/`useAiConfig` +`proxyUrl`锛沗SettingsPanel` AI 鍖?+"缃戠粶浠ｇ悊" Field銆?
- **楠岃瘉锛?* `cargo check` PASS锛坮eqwest socks 缂栬瘧 3.66s锛夛紱`npx tsc --noEmit` PASS銆?
- **閬楃暀锛?* `proxy_url` 鏄庢枃瀛橈紙浠ｇ悊鍦板潃闈炴晱鎰燂紱璁よ瘉鍐?url 鍐咃級锛涘闇€鐙珛璁よ瘉瀛楁鍚庣画鍔犮€?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 26 complete锛圓I 缃戠粶浠ｇ悊锛宑argo check + tsc 缁匡級 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴锛氬～浠ｇ悊锛堝 http://127.0.0.1:7890锛夆啋 AI 璇锋眰璧颁唬鐞?|
| 鐩爣鏄粈涔堬紵 | AI 鍔╂墜鏀寔 http/socks5 浠ｇ悊锛屽簲瀵规棤娉曠洿杩?Claude/OpenAI 鐨勭幆澧?|
| 鎴戝鍒颁簡浠€涔堬紵 | reqwest 浠ｇ悊鐢?`Proxy::all(url)`锛坰ocks5 闇€ `socks` feature锛夛紱url 鍐?`user:pass@host` 鏀寔璁よ瘉锛汼QLite 鍔犲垪瀵瑰凡瀛樺湪琛ㄧ敤 `column_exists` + `ALTER`锛圕REATE IF NOT EXISTS 涓嶅姞鍒楋級 |
| 鎴戝仛浜嗕粈涔堬紵 | 4 鏂囦欢鍚庣锛圕argo/db/ai/main锛? 3 鏂囦欢鍓嶇锛坅pi/hook/Settings锛夊崗璋冨姞 proxy_url锛宑argo check + tsc 鍏ㄧ豢 |

### 闃舵 27锛氬叏閲忓畨鍏?+ 閫昏緫 + 鍐椾綑浠ｇ爜瀹¤涓庝慨澶嶏紙2026-06-22锛?
- **闇€姹傦細** 鐢ㄦ埛瑕佹眰鎺掓煡瀹夊叏/閫昏緫婕忔礊涓庡啑浣?鏃犳晥浠ｇ爜骞跺鐞嗐€?
- **鏂规硶锛?* 骞惰娲?4 涓笓瀹?agent锛坰ecurity-reviewer / rust-reviewer / typescript-reviewer / refactor-cleaner锛夊叏閲忔壂 Rust + TS锛屾眹鎬诲幓閲嶅悗閫愰」浜哄伐鏍稿疄琛屽彿鍐嶄慨銆?
- **鍏抽敭鐜纭锛?* `rustup`/`cargo 1.96.0` 瀹為檯宸茶锛圕LAUDE.md銆屾湭瑁呫€嶈繃鏃讹級锛屾晠 Rust 鏀瑰姩鍙紪璇戦獙璇侊紱渚濊禆 `log`/`chrono`/`rand` 鍧囧湪锛屾棤 `zeroize`/`shell-words`/`scopeguard`銆?
- **淇锛堝畨鍏級锛?*
  - `main.rs` IPC 缁撴瀯浣撶被鍨嬫紓绉伙紙**绾夸笂 bug**锛夛細`CommandHistoryItem`/`QuickCommandItem`/`QuickCommandExecItem` 鍔?`#[serde(rename_all="camelCase")]`銆傛鍓?wire 鍙?snake_case 鑰屽墠绔寜 camelCase 璇伙紝瀵艰嚧蹇嵎鍛戒护 `isGlobal`/`sortOrder` 鍏?`undefined`锛堝垎缁勯敊涔?+ 鎺掑簭澶辨晥锛夈€俙ConnectionConfig`/`FileEntry` 缁存寔 snake_case锛堝墠绔湁鎰忓姝わ紝娉ㄩ噴鏄庣ず锛屼笉鍔級銆?
  - `main.rs` 鎷掔粷绌哄瘑鐮?SSH 璁よ瘉锛歬eyring 缂洪」鏃跺師璧?`unwrap_or_default()` 鍙戠┖涓诧紙鍙兘瑙﹀彂鏈嶅姟鍣ㄨ处鎴烽攣瀹氾級锛屾敼涓鸿繑鍥炴槑纭敊璇€?
  - `main.rs` `read_text_file`/`read_file_base64` TOCTOU锛氭敼銆屽紑涓€娆?+ 鍙ユ焺 `metadata()` + `Take` 闄愰暱璇汇€嶏紝鏉滅粷璺緞浜屾瑙ｆ瀽琚帀鍖?澧為暱缁曡繃灏哄涓婇檺銆?
  - `backup.rs` 鍥炴粴鐗堟湰璺緞绌胯秺锛堢旱娣遍槻寰★級锛歚is_valid_version` 浠呮斁琛?`^\d+(\.\d+)*$`锛涘洖婊?marker 鍘熷啓 `"X (rolled back)"` 姘镐笉绛変簬 `APP_VERSION` 鑷存瘡娆″惎鍔ㄩ兘澶囦唤锛屾敼鍐?`APP_VERSION`銆?
  - `db.rs` `rename_folder`/`folder_has_children` LIKE 閫氶厤绗︽敞鍏ワ細鍚?`%`/`_` 鐨勬枃浠跺す鍚嶄細璇尮閰嶅苟鏀瑰啓 `group_path`锛堟暟鎹崯鍧忥級锛屾敼 `like_prefix_pattern` 杞箟 + `ESCAPE '\'`銆?
  - `vault.rs` `LockoutState::save` 鍘熷瓙鍖栵細tmp+rename锛堝敮涓€涓存椂鍚嶉伩骞跺彂 rename 涓㈠け锛夛紝闃插穿婧冪暀鍗婃埅鏂囦欢瑙ｆ瀽涓?default 闈欓粯閲嶇疆鏆村姏鐮磋В璁℃暟銆?
  - `proxy.rs` 浠ｇ悊鐩爣 host 鏍￠獙锛氭嫤 `\r\n`/鎺у埗绗?绌虹櫧锛岄槻 `CONNECT`/`Host` 澶磋姹傝蛋绉併€?
- **淇锛堥€昏緫/鍋ュ．鎬э級锛?*
  - `db.rs` `add_command_history` 鍘婚噸鍔?`AND pinned=0`锛岄槻閲嶈窇宸茬疆椤跺懡浠ゆ椂闈欓粯涓㈢疆椤躲€?
  - `ssh.rs` `exec_once` 杈撳嚭鍔?4 MiB 涓婇檺锛堥槻 `yes`/`cat /dev/zero` 鍦?20s 鎺㈡祴绐楀唴 OOM锛夛紝涓庝氦浜掗€氶亾 `append_capped` 瀵归綈銆?
  - `ai.rs` `truncate` 姝ヨ繘鍒?UTF-8 瀛楃杈圭晫锛堝師 `&s[..max]` 鍦?CJK 閿欒浣撲笂 panic锛夛紱`inspect_health_local` 鍔?20s 瓒呮椂锛堝師 PowerShell 鎸傝捣鍒?UI 姘歌浆锛夈€?
  - `ftp.rs` 鍒犳墜鍐?`days_to_ymd`锛屾敼 `chrono`锛堜笌 `vault.rs` 鍚屾鍙嶆ā寮忥紝娉ㄩ噴宸茶绀猴級銆?
  - 鍓嶇 `cmd-buffer.ts` 澶勭悊 UTF-16 浠ｇ悊瀵癸紙鍘?emoji/鐢熷兓 CJK 鎷嗘垚 lone surrogate 姹℃煋鍛戒护鍘嗗彶锛夈€?
  - `zmodem-bridge.ts` `joinPath` 鎷?`""`/`.`/`..` 鍙跺瓙锛堥槻閫冨嚭涓嬭浇鐩綍锛夛紱ZMODEM 鍙戦€佸け璐ョ敱銆屼粎 console.error銆嶆敼涓?`this.abort()`锛岄槻 UI 鏃犻檺杞湀銆?
  - `CommandBar.tsx` 杈撳叆鑱氱劍鏀圭敤 `containerRef` 闄愬畾鏈粍浠讹紙鍘?`document.querySelector` 鍦ㄥ tab 鏃惰仛鐒﹀埌棣栦釜 tab锛夈€?
  - `App.tsx` 鎸佷箙鍖?`aiPanelWidth` 璇诲彇鏃?clamp 鍒伴潰鏉胯嚜韬?[300,720]锛堝師瓒婄晫鍊艰嚧闈㈡澘涓嶅彲鐢級銆?
  - `SettingsPanel.tsx` 瀵煎叆/瀵煎嚭瀵嗙爜鏀?`finally` 娓呴浂锛堝師澶辫触璺緞娈嬬暀锛夛紱鑷畾涔変富棰?hex 鍏?`normHex` 褰掍竴涓?`#RRGGBB` 鍐嶆嫾 alpha 鍚庣紑锛堝師 `#RGB`/`#RRGGBBAA` 鎷煎嚭鏃犳晥 CSS 琚潤榛樹涪寮冿級銆?
- **娓呯悊锛堝啑浣?姝讳唬鐮侊級锛?* 鍒?`components/PassphraseDialog.tsx`锛堥浂寮曠敤鏁存枃浠讹級锛涘垹 `api.ts` 姝诲鍑?`lockVault`/`onSshExit`/`SshExitPayload`锛堝悗绔?`lock_vault` 鍛戒护淇濈暀锛屽睘鍙湭鏉ユ帴绾跨殑鑳藉姏闈級锛涘垹 `TerminalPanel.tsx` 绌?`forEach`锛涘垹 `Sidebar.tsx` 涓ゅ鍙啓涓嶈鐨?`dataset.connId`锛沗ConnectionDialog.tsx` 鍘熷 `invoke("read_text_file")` 鏀圭敤鏂板鐨勭被鍨嬪寲 `readTextFile` 鍖呰锛堣ˉ榻愪笁澶勫悓姝ョ害瀹氾級銆?
- **楠岃瘉锛?* `cargo check` PASS锛? warning锛夛紱`npx tsc --noEmit` PASS锛? error锛夈€?
- **閬楃暀/鏈鐞嗭紙璇勪及鍚庢湁鎰忎笉鍔級锛?* FTP 鏄庢枃锛團TPS 鏈疄鐜帮紝灞炰骇鍝佸喅绛栵紝闈炰唬鐮佺己闄凤級锛沗shell_path` 浠绘剰鍙墽琛岋紙鏈湴缁堢濞佽儊妯″瀷涓虹敤鎴疯嚜浼わ紝涓旀敼涔嬩激 UX锛屼粎娉ㄩ噴鎻愮ず锛夛紱`get_connection_password` 鏄庢枃鍥炰紶娓叉煋灞?+ AI `base_url` SSRF锛堝▉鑳佹ā鍨嬩负銆屾覆鏌撳眰琚敾闄枫€嶏紝妗岄潰鑷湁 UI 椋庨櫓杈冧綆锛岄渶浜у搧绾х‘璁ゅ悗鍐嶅仛 scheme 鐧藉悕鍗?閲嶈璇侊級锛涙湰鍦扮粓绔緭鍑烘棤 cap锛坸term 婊氬姩缂撳啿鑷湁涓婇檺锛屼笖涓虹敤鎴疯嚜浼?DoS锛屽姞鍚堝苟浼氬紩鍏ュ埛灞忓欢杩燂紝鎬т环姣斾綆锛夛紱SFTP 姣忚皟鐢ㄦ柊寮€ channel锛堝凡鐭ヨ璁″彇鑸嶏級锛沗channel.wait()` 鍙栨秷瀹夊叏锛堝凡娉ㄩ噴鏍囨敞锛屽緟楂樻祦閲忚瀵燂級銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 27 complete锛堝畨鍏?閫昏緫+鍐椾綑瀹¤淇锛宑argo check + tsc 鍙岀豢锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴锛氬揩鎹峰懡浠ゅ垎缁?鎺掑簭鍥炲綊姝ｅ父銆佸鍏ュ鍑?鍥炴粴/閲嶅懡鍚嶆枃浠跺す/鏈湴宸℃璺緞鏃犲洖褰?|
| 鐩爣鏄粈涔堬紵 | 娓呴櫎瀹夊叏婕忔礊涓庣嚎涓?bug锛堝揩鎹峰懡浠ょ被鍨嬫紓绉伙級銆佽ˉ榻愬仴澹€с€佸垹鍐椾綑浠ｇ爜锛屽叏绋嬮浂缂栬瘧/绫诲瀷鍥炲綊 |
| 鎴戝鍒颁簡浠€涔堬紵 | IPC 缁撴瀯浣撳懡鍚嶄袱杈瑰繀椤诲悓妗堬紙snake/camel 鍒贩鐢紝鍚﹀垯 TS 鎺ュ彛楠椾汉銆佽繍琛屾椂 undefined锛夛紱`metadata(path)`+`read(path)` 鏄?TOCTOU锛岃寮€涓€娆″彇鍙ユ焺鍏冩暟鎹紱SQLite `LIKE` 鐨?`%`/`_` 鍦ㄧ敤鎴峰彲鎺т覆閲岃 `ESCAPE`锛涘師瀛愬啓 lockout 绛夊皬鏂囦欢鍒敤瑁?`fs::write`锛堝穿婧冪暀鍗婃埅=闈欓粯閲嶇疆瀹夊叏璁℃暟锛夛紱rustup 瀹炲凡瑁咃紝CLAUDE.md 璇ユ潯宸茶繃鏃?|
| 鎴戝仛浜嗕粈涔堬紵 | 4 agent 骞惰鎵?鈫?浜哄伐鏍稿疄 鈫?淇?12 澶勫悗绔?+ 12 澶勫墠绔?+ 鍒?1 鏂囦欢/3 姝诲鍑猴紝cargo check锛? warn锛? tsc锛? err锛夊叏缁匡紝progress/README 鍚屾 |

### 闃舵 28锛? 椤逛綋楠屼紭鍖栵紙杩炴帴鎺掑簭纭 / 骞挎挱鍘婚噸 / 缁堢閫夊尯鍙鎬?/ Windows 浠诲姟鏍忓浘鏍囷級锛?026-06-22锛?
- **闇€姹傦細** 鐢ㄦ埛鎻?4 鐐癸細鈶犳枃浠跺す涓嬭繛鎺ユ槸鍚︽寜鍚嶆帓搴?鈶″悓杩炴帴寮€涓?tab 鏃跺箍鎾笉搴旈噸澶嶅彂 鈶㈢粓绔€変腑鍐呭闅句互鍒嗚鲸锛堟棤閫変腑鑹插彉鍖栵級鈶ｆ墦鍖呭悗浠诲姟鏍忔樉绀洪粯璁ゅ浘鏍囷紙exe 鍥炬爣姝ｅ父锛夈€?
- **鏀瑰姩锛?*
  - **鈶犺繛鎺ユ帓搴忊€斺€旂粡鏍稿疄宸插疄鐜帮紝鏃犻渶鏀瑰姩銆?* `Sidebar.tsx:buildTree` 绗?70-75 琛?`sortRec` 宸插 `n.children`锛堝瓙鏂囦欢澶癸級涓?`n.conns`锛堣繛鎺ワ級鎸?`name.localeCompare(..., "zh")` 鎺掑簭锛沗:219` `useMemo(buildTree)` 閲嶅缓锛沗:332-333` 闈炴悳绱㈡€?`walk(tree)` 娓叉煋鐨勬槸鎺掑簭鍚庣殑鏍戙€傜粨璁猴細鏂囦欢澶逛笅杩炴帴宸叉寜鍚嶆帓搴忥紙涓枃 locale锛夈€?
  - **鈶″箍鎾幓閲嶏紙`App.tsx:getBroadcastTargets`锛夛細** 閬嶅巻骞挎挱缁勬垚鍛樻椂鎸?`tab.connectionId` 鍘婚噸鈥斺€斿悓杩炴帴寮€涓?tab 鍙彇棣栦釜 session锛岄伩鍏嶅鍚屼竴鍙版湇鍔″櫒閲嶅鍙戝悓涓€鎸夐敭锛堝弻鎵ц锛夈€傛棤 connectionId 鐨?tab 浠嶇収甯哥撼鍏ャ€?
  - **鈶㈢粓绔€夊尯鍙鎬э紙`TerminalPanel.tsx`锛夛細** 鏍瑰洜鈥斺€斿悇涓婚 `selectionBackground` 澶氱敤浣?alpha锛坄#RRGGBBAA` 鏈袱浣?`44`鈮?7% / `88`鈮?3%锛夛紝鍦ㄧ粓绔儗鏅笂鍑犱箮鐪嬩笉鍑恒€傛柊澧?`visibleSelection()` 鎶?alpha 鎻愬埌 `cc`锛堚増80%锛屼繚鐣欏悇涓婚鍘熸湁鑹茬浉锛夛紝鍦?Terminal 鏋勯€犲锛坄:143`锛変笌閲嶆覆鏌撳锛坄:408`锛変袱澶?`theme` spread 娉ㄥ叆锛岃鐩栨櫘閫?鑳屾櫙鍥句袱绉嶆覆鏌撹矾寰勩€?
  - **鈶indows 浠诲姟鏍忓浘鏍囷紙`main.rs` + `Cargo.toml`锛夛細** exe 鍥炬爣姝ｅ父 鈬?tauri-build 宸叉纭祵鍏ュ浘鏍囪祫婧愶紱浠诲姟鏍忎粛鏄粯璁?鈬?韬唤/鍒嗙粍闂銆傚弻淇濋櫓锛歛) `run()` 璧峰澶?`set_windows_app_user_model_id()` 缁?shell32 鍐呰仈 FFI 璋?`SetCurrentProcessExplicitAppUserModelID("com.myshell.client")`锛堜笌 tauri.conf identifier 鍚屽€硷級锛屽繀椤诲湪绐楀彛鍒涘缓鍓嶈缃紱b) `Cargo.toml` tauri 鍔?`image-ico` feature锛宍.setup()` 閲?`window.set_icon(Image::from_bytes(include_bytes!("../icons/icon.ico")))` 鏄惧紡缁欎富绐楀彛涓婂浘鏍囥€傛敞鎰?Tauri 2.x 璇ョ増鏈?`set_icon` 鐩存帴鍚?`Image` 闈?`Option<Image>`锛堝垵鐗堝啓 `Some(icon)` 缂栬瘧鎶?E0308锛屽凡鏀癸級銆?
- **楠岃瘉锛?* `cargo check` PASS锛? warn锛宼auri 鍔?image-ico 閲嶇紪 ~50s锛夛紱`npx tsc --noEmit` PASS锛? err锛夈€?
- **閬楃暀锛?* 鈶ｉ渶鐢ㄦ埛閲嶆柊 `cargo tauri build` 鍑哄寘楠岃瘉锛涜嫢浠诲姟鏍忎粛鏄剧ず鏃ч粯璁ゅ浘鏍囷紝澶氫负 Windows 鍥炬爣缂撳瓨锛堜换鍔℃爮宸插浐瀹氬揩鎹锋柟寮忥級/闇€鍙栨秷鍥哄畾閲嶅浐瀹氾紝鎴?`ie4uinit.exe -show` 鍒锋柊鍥炬爣缂撳瓨銆傗憼纭鏃犻渶鏀瑰姩銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 28 complete锛? 椤逛紭鍖栵細鎺掑簭纭/骞挎挱鍘婚噸/閫夊尯鍙/浠诲姟鏍忓浘鏍囷紝cargo + tsc 鍙岀豢锛?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴閫夊尯楂樹寒 + 骞挎挱鍘婚噸锛涘嚭鍖呴獙璇佷换鍔℃爮鍥炬爣 |
| 鐩爣鏄粈涔堬紵 | 瑙ｅ喅鐢ㄦ埛鎻愮殑 4 涓綋楠岀偣锛岄浂鍥炲綊 |
| 鎴戝鍒颁簡浠€涔堬紵 | xterm `selectionBackground` 鏀寔 8 浣?hex `#RRGGBBAA`锛宎lpha 澶綆鍒欓€夊尯涓嶅彲瑙侊紱Windows銆宔xe 鍥炬爣瀵广€佷换鍔℃爮榛樿銆嶅嚑涔庨兘鏄己 AUMID锛坄SetCurrentProcessExplicitAppUserModelID`锛岄』绐楀彛鍒涘缓鍓嶈锛夛紱Tauri 2.x `WebviewWindow::set_icon` 绛惧悕鏄?`Image` 鑰岄潪 `Option<Image>`锛堢増鏈浉鍏筹紝鎶ラ敊涓哄噯锛夛紱`#[link(name="shell32")] extern "system"` 鍙厤 winapi feature 鐩存帴璋冪郴缁?API |
| 鎴戝仛浜嗕粈涔堬紵 | 鈶犵‘璁ゅ凡鎺掑簭 鈶etBroadcastTargets 鎸?connectionId 鍘婚噸 鈶isibleSelection 鎻?alpha 娉ㄥ叆涓ゅ 鈶UMID + set_icon 鍙屼繚闄╋紙+image-ico feature锛夛紝cargo/tsc 鍙岀豢锛宲rogress/README 鍚屾 |

### 闃舵 29锛歋FTP 鎵撳紑鍗虫姤 "Read dir failed: No such file" + 鐗堟湰鍙?1.4.2锛?026-06-22锛?
- **闇€姹傦細** 鈶犵増鏈彿鏀?1.4.2锛涒憽杩炴帴 `sftp_perceptualCenter@...` 鍚?SFTP 闈㈡澘鐩存帴鎶?`Read dir failed: No such file: No such file`锛岀敤鎴峰弽棣堛€岃繛鎺?sftp 寮傚父銆嶃€?
- **鏍瑰洜锛堚憽锛夛細** `SftpPanel.tsx` 鍒濆璺緞鐢?`~`锛圫SH/SFTP 鎯敤鐨?home 蹇嵎锛夛紝浣?**SFTP 鍗忚 + russh-sftp 鎶?`~` 褰撳瓧闈㈢洰褰曞悕**鈥斺€旀病鏈?shell 鍋氬睍寮€锛屾湇鍔″櫒涓婂張娌℃湁鍚嶄负 `~` 鐨勭洰褰曪紝浜庢槸 `read_dir("~")` 杩斿洖 `SSH_FX_NO_SUCH_FILE`锛宍format!("Read dir failed: {}", e)` 鎶?russh-sftp 鐨勩€岀姸鎬佺爜鍚? 鏈嶅姟鍣ㄦ秷鎭€嶆覆鏌撴垚 `No such file: No such file`锛堟墍浠ラ噸澶嶄袱娆★級銆備笌 SFTP 瀛愮郴缁熸槸鍚﹀彲鐢ㄣ€乻hell 閫氶亾鏄惁绉掗€€锛圗xitStatus=1锛岃璐﹀彿鐤戜技 nologin/chroot 鐨?SFTP-only 璐﹀彿锛夋棤鍏斥€斺€斿瓙绯荤粺閫氶亾寮€寰楄捣鏉ワ紝閿欏湪璺緞銆?
- **鏀瑰姩锛?*
  - `src-tauri/src/sftp.rs` 鏂板 `resolve_path(sftp, path)`锛氶亣 `~` / `~/` / `~/foo` 鏃跺厛 `sftp.canonicalize(".")`锛圫FTP REALPATH锛岃繑鍥炴湇鍔″櫒榛樿鐩綍=home锛夊啀鎷煎悗缂€锛涚粷瀵?鐩稿璺緞鍘熸牱閫忎紶銆俙list_dir` / `create_dir` / `remove` / `rename` 鍥涘鍏ュ彛缁熶竴璧板畠銆俙list_dir` 鐨勫瓙椤?`full_path` 鏀圭敤瑙ｆ瀽鍚庣殑缁濆璺緞锛屼娇浠?`~` 杩涗笅涓€绾у悗鍦板潃鏍忚嚜鐒跺彉鎴愮湡瀹炵粷瀵硅矾寰勶紙`/home/.../name`锛夛紝鑰岄潪鍋滅暀鍦?`~`銆?
  - 娉ㄦ剰鎵€鏈夋潈锛歚read_dir`/`remove_file`/`remove_dir` 閮芥槸 `P: Into<String>`锛屼紶 owned `String` 浼氳 move 鎺夈€佷箣鍚庡啀 `format!` 鐢ㄥ氨鎶?use-after-move锛涚粺涓€浼?`resolved.as_str()`锛坄&str: Into<String>`锛屽€熺敤涓?move锛夈€俙rename` 涓ゅ弬鍚?move 涓€娆°€佺敤鍚庝笉鍐嶈闂紝OK銆?
  - 鐗堟湰鍙?1.4.1 鈫?1.4.2锛氭敼 `Cargo.toml`锛堝敮涓€鐪熸簮锛夊悗璺?`npm run version:sync` 鍚屾鍒?`package.json` + `package-lock.json`锛沗tauri.conf.json` 鏃犻渶鐗堟湰瀛楁锛圱auri v2 鐩存帴璇?Cargo.toml锛夈€?
  - **闄勫甫淇锛堥樆鏂紪璇戠殑閬楃暀 typo锛夛細** `Cargo.toml` 閲?`rusqlite` 鐨?feature 鍐欐垚浜?`bundclaudled`锛堜笉瀛樺湪鐨?feature锛夛紝cargo 鐩存帴鎶?`does not have that feature` 鍏ㄩ噺缂栦笉杩囥€傛敼鍥炴纭殑 `bundled`锛堜笌 CLAUDE.md銆宺usqlite (bundled)銆嶄竴鑷达級銆傝 typo 姝ｆ槸浼氳瘽寮€濮嬫椂 `git status` 閲岄偅涓湭鎻愪氦鐨?`M src-tauri/Cargo.toml`銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛? err锛夛紱`cargo check` PASS锛? warn锛?3.8s锛夈€俙canonicalize`/`read_dir`/`rename`/`remove_file`/`remove_dir`/`create_dir` 绛惧悕鍧囧凡瀵圭収 russh-sftp 2.3.0 婧愮爜纭锛坄canonicalize` 鍦?`client/session.rs:127`锛宍read_dir` 杩斿洖鍙凯浠ｇ殑 `ReadDir`锛夈€?
- **閬楃暀 / 寰呯敤鎴峰疄娴嬶細** chroot 鍨?SFTP-only 璐﹀彿鑻?home 鍦?chroot 鍐呬笉瀛樺湪锛屾湇鍔″櫒浼氭妸 CWD 钀藉湪 `/`锛屾鏃?`canonicalize(".")` 杩斿洖 `/`銆佸垪 `/`锛屼笉浼氬啀鎶?No such file锛堥檷绾ф纭級銆傞渶鐢ㄦ埛 dev 瀹炴祴璇ヨ繛鎺ョ殑 SFTP 闈㈡澘鑳芥甯稿垪鐩綍銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 29 complete锛圫FTP `~` 灞曞紑淇 + 鐗堟湰 1.4.2 + 椤烘墜淇?rusqlite feature typo锛夛紝cargo + tsc 鍙岀豢 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴 `sftp_perceptualCenter` 杩炴帴鐨?SFTP 闈㈡澘鑳芥甯稿垪鐩綍銆佽繘閫€銆佸鍒犳敼鍚?|
| 鐩爣鏄粈涔堬紵 | 瑙ｅ喅銆岃繛鎺?sftp 寮傚父銆嶇嚎涓?bug锛岀増鏈彿鍗?1.4.2锛岄浂鍥炲綊 |
| 鎴戝鍒颁簡浠€涔堬紵 | SFTP 鍗忚鏃?shell锛宍~` 涓嶅睍寮€鈥斺€旀墍鏈?`~` 閮藉緱鐢?REALPATH(canonicalize ".") 瑙ｆ瀽锛況ussh-sftp 鍚勬柟娉曞娉涘瀷 `Into<String>`锛屼紶 owned String 浼?move锛岃 `.as_str()`锛況ussh-sftp 閿欒鏄剧ず鏄€岀姸鎬佺爜鍚? 鏈嶅姟鍣ㄦ秷鎭€嶆晠 No such file 閲嶅涓ゆ锛涚増鏈彿鍗曚竴鐪熸簮鍦?Cargo.toml锛宻ync-version.mjs 璐熻矗鎵╂暎 |
| 鎴戝仛浜嗕粈涔堬紵 | sftp.rs 鍔?resolve_path 骞舵帴鍏ュ洓鍏ュ彛锛堝惈鎵€鏈夋潈淇锛夈€佺増鏈?1.4.1鈫?.4.2锛?sync锛夈€佷慨 `bundclaudled`鈫抈bundled` 缂栬瘧闃绘柇 typo锛宑argo/tsc 鍙岀豢锛宲rogress/README 鍚屾 |

### 闃舵 30锛歋FTP 鐐瑰嚮鐩綍鎶?"SSH session not found" 鈥斺€?shell 閫氶亾鍏抽棴璇垹鏁翠釜 session锛?026-06-22锛?
- **闇€姹傦細** 闃舵 29 淇畬 `~` 灞曞紑鍚庯紝鐢ㄦ埛瀹炴祴 SFTP 闈㈡澘鑳借繘鍏ワ紙鍒?home 鎴愬姛锛夛紝浣嗐€岀偣鍑荤洰褰曞悗鎻愮ず SSH session not found銆嶃€?
- **鏍瑰洜锛?* `channel_reader` 鍦?shell **閫氶亾**鍏抽棴鏃讹紙EOF/Close/`exit`/ExitStatus锛夋墽琛?`sessions.remove(&session_id)`锛屾妸鏁翠釜 `SshSession` 杩炲悓 `Arc<Handle>` 涓€璧蜂粠 map 鍒犳帀銆備絾 russh 鏄€屼竴鏉?SSH 杩炴帴澶氳矾澶嶇敤澶氫釜閫氶亾銆嶁€斺€攕hell 閫氶亾姝讳簡 鈮?SSH 杩炴帴姝讳簡锛孲FTP 瀛愮郴缁熼€氶亾鏄?shell 閫氶亾鐨勫厔寮燂紝闈犲悓涓€涓?`Arc<Handle>` 寮€鏂伴€氶亾銆傝璐﹀彿鏄?SFTP-only锛坣ologin/chroot锛夛紝shell 绉掗€€锛圗xitStatus=1锛夆啋 reader 閫€鍑?鈫?鍒?session 鈫?鍚庣画鎵€鏈?SFTP 璋冪敤 `get_sftp_session` 鎵句笉鍒?session 鈫?"SSH session not found"銆傜涓€娆″垪鐩綍鑳芥垚鍔熷彧鏄洜涓洪潰鏉挎寕杞芥椂鎶㈠湪 reader 閫€鍑哄墠瀹屾垚浜?`read_dir`銆?
- **鏀瑰姩锛?*
  - `src-tauri/src/ssh.rs`锛?
    - `channel_reader` 涓嶅啀鎺ユ敹 `sessions` 鍙傛暟銆佷笉鍐嶅湪閫€鍑烘椂 `map.remove`锛堝垹 `HashMap` import锛沗Mutex` 浠嶈 `SshClient.db` 鐢紝淇濈暀锛夈€傛敼涓哄彧鍦?loop 鍐?emit `ssh_closed` 鍚庨€€鍑恒€俿hell 閫氶亾鍏抽棴鍙槸銆岀粓绔柇浜嗐€嶏紝session锛堣繛鎺ワ級淇濈暀渚?SFTP/exec 缁х画寮€鏂伴€氶亾銆?
    - `disconnect()` 鎴愪负鍞竴鍒?session 鐨勭偣锛歚sessions.remove(session_id)` 鍙栧嚭鎵€鏈夋潈 鈫?缁?reader 鍙?`Disconnect`锛坆est-effort锛孲FTP-only 璐﹀彿 reader 宸查€€锛宯o-op锛夆啋 鍑芥暟缁撴潫 drop `SshSession` 鈫?drop `Arc<Handle>` 鈫?鍏抽棴 SSH 杩炴帴锛堜换浣曞湪椋炵殑 SFTP `Arc::clone` 閲婃斁鍚庯級銆俙ssh::disconnect` 鍥犳鍙?idempotent锛坰ession 涓嶅湪 map 杩斿洖 Ok锛夈€?
    - `connect()` 鍚屾鍘绘帀 `sessions_arc` 鐨?clone 涓庝紶鍙傘€?
  - `src/App.tsx::handleCloseTab`锛氬師鏉ヤ粎鍦?`tab.status === "connected"` 鏃舵柇寮€銆備絾 SFTP-only 璐﹀彿 shell 绉掗€€鍚?`onSshClosed 鈫?onDisconnected` 鎶?status 缃?"disconnected"锛屽叧 tab 鏃惰烦杩?`sshDisconnect` 鈫?session 娉勬紡銆傛敼涓猴細SSH/SFTP 鍒嗘敮鏃犳潯浠?`sshDisconnect`锛堜緷璧栧悗绔?idempotent锛夛紱FTP/local 浠嶅彧鍦?connected 鏃舵柇锛堜簩鑰呴潪 idempotent / 娓呯悊妯″瀷涓嶅悓锛夈€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛? err锛夛紱`cargo check` PASS锛? warn锛?.4s锛夈€?
- **琛屼负褰卞搷锛堟闈級锛?* 鏅€氳处鍙峰湪 shell 閲屾暡 `exit` 鍚庯紝SFTP 闈㈡澘浠嶅彲鐢紙杩炴帴鏈柇锛屽彧鏄?shell 閫氶亾鍏充簡锛夆€斺€斾笌 FinalShell 绛変竴鑷淬€傜湡姝ｆ柇寮€鐨勮繛鎺ヤ細鐣欏湪 map 閲岀洿鍒扮敤鎴峰叧 tab锛堢粓绔凡鏄剧ず [Connection closed]锛屽姝?handle 鐨?SFTP 璋冪敤杩斿洖娓呮櫚閿欒鑰岄潪宕╂簝锛夛紝鍙帴鍙椼€?
- **閬楃暀 / 寰呭疄娴嬶細** 闇€鐢ㄦ埛 dev 瀹炴祴璇ヨ繛鎺ワ細鈶犺繘 SFTP 闈㈡澘鑳芥甯歌繘閫€鐩綍銆佸鍒犳敼鍚?鈶″叧 tab 鍚?session 琚竻鐞嗭紙鏃犳硠婕忥級銆俛pp 閫€鍑烘椂 `drain_all_sessions` 浠嶅彧鍙?Disconnect 涓嶆竻 map锛堜緷璧?OS 鍥炴敹 TCP锛屼笌 FTP 涓€鑷达紝main.rs drain_all_sessions 涓婃柟娉ㄩ噴宸茶鏄庯級銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 30 complete锛坰ession 鐢熷懡鍛ㄦ湡涓?shell 閫氶亾瑙ｈ€?+ 鍏?tab 娓呯悊锛夛紝cargo + tsc 鍙岀豢 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 dev 瀹炴祴 SFTP 杩涢€€/澧炲垹鏀瑰悕 + 鍏?tab 鏃犳硠婕?|
| 鐩爣鏄粈涔堬紵 | 褰诲簳淇ソ SFTP-only 璐﹀彿銆岃繛鎺?sftp 寮傚父銆嶏紝闆跺洖褰?|
| 鎴戝鍒颁簡浠€涔堬紵 | russh 涓€鏉¤繛鎺ュ璺鐢ㄥ閫氶亾锛宻hell 閫氶亾姝?鈮?杩炴帴姝伙紝session 鐢熷懡鍛ㄦ湡涓嶈兘缁戞鍦?shell reader 涓婏紱鍒?`HashMap` import 鍓嶈鍏堢‘璁ゆ病琚?`SshClient.db` 涔嬪鐨?Mutex 澶嶇敤锛涙敼鍚庣 cleanup 璇箟瑕侀『甯﹀鍓嶇鐨勫叧 tab / status 闂ㄦ帶锛屽惁鍒欍€屽悗绔繚鐣?session銆佸墠绔烦杩?disconnect銆嶄細娉勬紡 |
| 鎴戝仛浜嗕粈涔堬紵 | channel_reader 鍘绘帀 sessions 鍙傛暟涓?map.remove锛?鍒?HashMap import锛夛紱disconnect() 鏀逛负鍞竴鍒?session 鐐瑰苟 idempotent锛汚pp.tsx 鍏?tab 鏃?SSH/SFTP 鏃犳潯浠舵柇寮€锛沜argo/tsc 鍙岀豢锛宲rogress/README 鍚屾 |

### 闃舵 31锛歋FTP 鎵归噺鏂囦欢涓婁紶/涓嬭浇锛?026-06-22锛?
- **闇€姹傦細** SFTP 闈㈡澘褰撳墠鍙湁 鍒犻櫎 / 鏂板缓鏂囦欢澶?/ 閲嶅懡鍚嶏紝缂哄皯鏂囦欢涓婁紶鍜屼笅杞借兘鍔涖€傞渶瑕佹敮鎸佹壒閲忓閫夋枃浠朵笂浼犮€佹壒閲忎笅杞藉埌鎸囧畾鏈湴鐩綍銆傛枃浠跺す閫掑綊鍙笉鍋氾紙YAGNI锛寁1 涓嶅惈锛夛紝瑕嗙洊绛栫暐閲囩敤鐩存帴瑕嗙洊锛堟渶绠€鍗曪級銆?
- **鏀瑰姩锛?*
  - `src-tauri/src/sftp.rs`锛堟柊澧?~210 琛岋級锛?
    - `upload(state, sid, local_paths: Vec<String>, remote_dest_dir, request_id, window)` 鈥?棰?stat锛坄tokio::fs::metadata` 鍙?size/total銆佽烦杩囩洰褰曪級銆侀『搴忎紶杈撴瘡涓枃浠讹紙`sftp::create` = CREATE|TRUNCATE|WRITE 鈫?32KB 缂撳啿寰幆 `AsyncReadExt::read`鈫抈AsyncWriteExt::write_all`锛夈€乣flush()` 椹卞姩 write acks 瀹屾垚锛坮ussh-sftp 2.3 `File` 鏃犳樉寮?`close()`鈥斺€擿Drop` impl 鐢?`close_nowait` 閲婃斁 handle锛夈€佽繘搴?emit 鑺傛祦 120ms銆佸崟鏂囦欢澶辫触璁板綍鍚庣户缁笅涓€涓€佹敹灏?`sftp_transfer_done`銆?
    - `download(state, sid, remote_paths: Vec<String>, local_dest_dir, request_id, window)` 鈥?瀵圭О锛歚sftp::open` = READ 鈫?`metadata().size` 绱姞 鈫?`tokio::fs::create_dir_all` 鈫?32KB 寰幆 `file.read`鈫抈local.write_all` 鈫?`flush()`銆?
    - 涓変釜浜嬩欢缁撴瀯浣擄紙camelCase serde锛夛細`TransferProgressPayload`銆乣TransferDonePayload`銆乣TransferErrorPayload`锛堝悗鑰呭湪瀹炵幇涓湭琚洿鎺ユ瀯閫犫€斺€旇嚧鍛介敊璇敤 `sftp::upload`/`download` 杩斿洖 `Err` 鐢卞墠绔?`.catch` 澶勭悊鈥斺€斿垹鎺変簡浠ユ秷闄?warning锛夈€?
    - 杈呭姪鍑芥暟 `basename(path)`锛堢敤 `std::path::Path::file_name` 闃茶矾寰勯€冮€革級銆乣emit_transfer_progress`銆?
    - 閫氶亾澶嶇敤锛氫竴涓?`SftpSession`锛堜竴鏉″瓙绯荤粺閫氶亾锛夊鐞嗘暣鎵规枃浠讹紝鍚勬枃浠跺悇鑷?open/close handle锛屽紑閿€鍙帶銆?
  - `src-tauri/src/main.rs`锛?
    - `sftp_upload` / `sftp_download` 涓や釜 `#[tauri::command]` 鍖呰锛坄WebviewWindow` 娉ㄥ叆锛屽悓 `ssh_connect` 鑼冨紡锛夈€?
    - `generate_handler!` 娉ㄥ唽锛堢揣璺?`sftp_rename` 涔嬪悗锛夈€?
  - `src/api.ts`锛?
    - `sftpUpload(sessionId, localPaths, remoteDestDir, requestId)` / `sftpDownload(sessionId, remotePaths, localDestDir, requestId)` invoke 鍖呰銆?
    - 涓変釜 requestId 杩囨护浜嬩欢鐩戝惉鍣細`onSftpTransferProgress`銆乣onSftpTransferDone`銆乣onSftpTransferError`锛堝悗鑰呴厤濂楀垹闄ょ殑鍚庣浜嬩欢锛夈€?
    - 瀵煎嚭绫诲瀷 `SftpTransferProgressPayload`銆乣SftpTransferDonePayload`銆?
  - `src/components/SftpPanel.tsx`锛堝叏闈㈡敼鍐欙級锛?
    - **澶氶€?*锛氭枃浠惰鍔?`checkbox`锛坄selected: Set<string>` 瀛?`entry.path`锛夛紝鍒囨崲鐩綍鏃舵竻绌洪€夊尯銆?
    - **宸ュ叿鏍?*锛氣瑔 涓婁紶鎸夐挳锛坄open({ multiple: true })` 閫夋湰鍦版枃浠?鈫?`runTransfer` 鈫?`sftpUpload`锛夛紱猬?涓嬭浇鎸夐挳锛堥€変腑鏂囦欢椤规暟 > 0 鏃跺惎鐢?鈫?`open({ directory: true })` 閫夌洰鏍囩洰褰?鈫?`sftpDownload`锛屼粎鏀堕泦 `!is_dir` 鐨勯€変腑椤癸紝鏂囦欢澶瑰悎鍚屼笉绾冲叆锛夈€?
    - **`runTransfer`**锛堜笂浼?涓嬭浇鍏变韩鐨勪簨浠剁敓鍛藉懆鏈燂級锛歚requestId = crypto.randomUUID()`锛沗setTransfer` 鍒濆鍖栫姸鎬?鈫?`onSftpTransferProgress` + `onSftpTransferDone` 璁㈤槄锛堝厛浜?invoke锛岄槻婕忔棭鏈熶簨浠讹級鈫?鍚姩璋冪敤 鈫?瀹屾垚鏍囪 `done: true` 鈫?鍥炶皟鍒锋柊鐩綍锛堜笂浼犳椂锛夈€?
    - **`TransferOverlay`**锛氬簳閮ㄧ粷瀵瑰畾浣嶆诞灞傦紝鏄剧ず闃舵鍚嶏紙涓婁紶涓?涓嬭浇涓?瀹屾垚锛夈€佸綋鍓嶆枃浠跺悕銆乣fileIndex/fileCount`銆佹€昏繘搴︽潯 `bytesDone/bytesTotal`锛堢櫨鍒嗘瘮 width transition锛夈€佸畬鎴愭€佸垪鍑哄墠 3 鏉?per-file 閿欒锛堣嫢鏈夛級锛涘叧闂寜閽竻鎺夌洃鍚櫒銆傚閮ㄧ偣鍑讳笉鍏抽棴锛堥槻璇Е涓柇鐢ㄦ埛璇婚敊璇鎯咃級銆?
    - 鍘熸湁宸ュ叿鏍?`ToolBtn` / 鏍煎紡鍖?/ 鐩綍鎿嶄綔绛変繚鎸佷笉鍙樸€?
    - 浠?SSH source 鏄剧ず涓婁紶/涓嬭浇鎸夐挳锛團TP source 涓?`display: none` 鐨勭畝骞垛€斺€旀湰娆℃湭鍔?FTP锛屼絾浜岃繘鍒跺鐢ㄥ悓涓€ `SftpPanel`锛夈€?
- **宸茶 russh-sftp 2.3.0 婧愮爜鏍稿锛坄~/.cargo/registry/src/鈥?russh-sftp-2.3.0/src/client/`锛夛細**
  - `session.rs:97` `create()` 鈫?`CREATE|TRUNCATE|WRITE` 鈥?纭瑕嗙洊璇箟姝ｇ‘銆?
  - `session.rs:90-91` `open()` 鈫?`OpenFlags::READ` 鈥?纭鍙銆?
  - `fs/file.rs:135-142` `Drop` 鈥?`close_nowait(handle)` 閲婃斁 handle锛涙棤鏄惧紡 `close()` 鏂规硶锛屽垵鐗堢紪璇戞姤 `no method close`锛屾敼涓轰粎 `flush()` + scope drop銆?
  - `fs/file.rs:260-294` `AsyncWrite::poll_write` 鈥?鏀寔骞跺彂 write ack锛宍flush()` 椹卞姩鍏ㄩ儴 ack 瀹屾垚銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛? err锛夛紱`cargo check` PASS锛? warn锛?.6s锛夈€傚璇濆唴涓柇鐨?`cargo check` 宸插湪缁細閲嶈窇纭銆?
- **閬楃暀 / 寰呭疄娴嬶細** `cargo tauri dev` 鎵嬫祴鈥斺€斾笂浼犲閫夋枃浠?鈫?杩涘害娴眰璧板畬 鈫?鐩綍鍒锋柊鍑虹幇鏂版枃浠讹紱涓嬭浇閫変腑澶氭枃浠?鈫?閫夌洰鏍囩洰褰?鈫?鏈湴寰楀埌杩欎簺鏂囦欢銆傝鐩栧凡鏈夋枃浠剁敓鏁堛€俙sftpTransferError`锛坒atal channel error锛夊湪 `runTransfer` 鐨?`.catch` 閲屽厹搴曪紝涓嶄竴瀹氭湁鍚庣浜嬩欢瑙﹀彂鈥斺€旇璁′笂鏄弻閲嶄繚闄┿€傞潪 SSH source 闈㈡澘鏃犳寜閽紝涓嶅仛澶氫綑鎿嶄綔銆?

### 闃舵 32锛氳繛鎺ョ被鍨嬪浘鏍囨浛鎹负 iconfont 瀛椾綋鍥炬爣锛?026-06-22锛?
- **闇€姹傦細** ftp / sftp / 鏈湴 / ssh 鐨勮繛鎺ュ浘鏍囧師鍏堢敤 emoji锛堭煋?馃搧/馃捇/馃枼锔忥級锛岃法骞冲彴娓叉煋涓嶄竴鑷淬€佷笉鍚岀郴缁熷瓧褰㈠樊寮傚ぇ銆傜敤鎴锋彁渚涗簡涓€浠?iconfont.cn 瀛椾綋鍖咃紙`G:/妗岄潰/font_tm10hhyy2ag`锛? 涓瓧褰細鐢佃剳/ftp/鏈嶅姟鍣?SFTP锛夛紝瑕佹眰鐢ㄨ繖濂楀瓧浣撳浘鏍囨浛鎹€?
- **鏀瑰姩锛?*
  - 鏂板 `src/assets/iconfont/iconfont.ttf` + `iconfont.css`锛堜粠瀛椾綋鍖呮嫹鍏ワ紱CSS 鏀逛负鐩稿璺緞 `url("./iconfont.ttf")`锛孷ite 鏋勫缓鏃?base64 鍐呰仈杩涙墦鍖?CSS锛夈€?
  - `src/styles/global.css` 椤堕儴 `@import "../assets/iconfont/iconfont.css";`锛堜粎寮曞叆涓€娆★級銆?
  - 鏂板 `src/components/ConnIcon.tsx`锛歚ConnIcon` 缁勪欢锛坄connType` 鈫?`icon-*` class锛宍size` 鎺у埗 font-size锛岄鑹茶蛋 `currentColor` 璇箟鍖栨槧灏?`CONN_COLOR`锛歴sh 涓昏摑 / sftp 鍓潚 / ftp 璀﹀憡榛?/ local 娆＄骇鐏帮級銆?
  - `Sidebar.tsx`锛氬垹 `CONN_ICONS` emoji map锛岃繛鎺ヨ + 绌虹姸鎬佺敤 `ConnIcon`銆?
  - `ConnectionDialog.tsx`锛歚TYPE_OPTIONS` 鍘绘帀 emoji `icon` 瀛楁锛宍TypeSelector` 鎸夐挳鐢?`ConnIcon`銆?
  - `TabBar.tsx`锛氭爣绛捐繛鎺ョ被鍨嬪浘鏍囩敤 `ConnIcon`锛堥鑹?`inherit`锛岃窡闅?tab 鏂囧瓧鑹诧級銆?
  - `QuickCommandsPanel.tsx` 涓嬫媺閲岀殑 `馃枼锔廯 鏄?`<option>` 鏂囨湰锛圖OM `<option>` 鏃犳硶鐢ㄥ瓧浣?class锛屽彧鑳芥斁绾枃鏈?emoji锛夆€斺€斾繚鐣欎笉鍔紝涓嶆浛鎹€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛? err锛夛紱`npx vite build` PASS锛圕SS 13.65kB 鍚?base64 瀛椾綋锛屾棤棰濆 ttf 浜х墿锛夈€?
- **璁捐鍙栬垗锛?* 棰滆壊璇箟鍖栭泦涓埌 `CONN_COLOR` 鍗曚竴婧愶紝涓夊璋冪敤鏂癸紙渚ф爮/瀵硅瘽妗?鏍囩鏍忥級棰滆壊涓€鑷达紱`color: inherit` 鍏佽璋冪敤鏂硅鐩栵紙濡傚璇濇閫変腑鎬佺敤涓昏壊锛夈€俙<option>` 涓嶆浛鎹㈡槸 DOM 闄愬埗锛岄潪閬楁紡銆?

### 闃舵 33锛歋FTP 杩炴帴涓€鐩存彁绀洪噸杩烇紙nologin 璐︽埛 shell 绔嬪嵆閫€鍑鸿Е鍙戣鎶?ssh_closed锛夛紙2026-06-22锛?
- **鐜拌薄锛?* 鎵撳紑涓€涓?SFTP 杩炴帴锛屽悗绔棩蹇楋細`connected to sftp_xxx@host` 鈫?`channel_reader started` 鈫?`ExitStatus=1` 鈫?`Data 44 bytes` 鈫?`EOF` 鈫?`channel_reader exited`銆傚墠绔?`SftpPanel` 鏀跺埌 `ssh_closed`锛岀姸鎬佸彉 `disconnected`锛屾樉绀恒€岃繛鎺ュ凡鏂紑 / 閲嶈繛銆嶉伄缃┿€備絾 SFTP 鏂囦欢鍒楄〃鍏跺疄鑳芥甯告媺銆?
- **鏍瑰洜锛?* 璇?SFTP 璐︽埛鏄?**nologin / SFTP-only** 璐︽埛锛堢櫥褰?shell 绔嬪嵆閫€鍑猴紝ExitStatus=1锛夈€俙ssh::connect` 鏃犳潯浠?`request_pty` + `request_shell`鈥斺€旇繖绉嶈处鎴风殑 shell 涓€鍚姩灏遍€€锛宍channel_reader` 璧板埌 `Eof` 鍒嗘敮 emit `ssh_closed`銆傝€?`SftpPanel`锛堜綔涓虹嫭绔?tab 娓叉煋鏃讹級璁㈤槄浜?`onSshClosed`锛圫ftpPanel.tsx:121-139锛夛紝鎶?shell 閫氶亾鍏抽棴璇垽鎴愭暣涓繛鎺ユ寕浜?鈫?寮归噸杩炪€傞樁娈?30 鐨勪慨澶嶅彧璁?`channel_reader` 涓嶅啀浠?map 鍒?session锛堟墍浠?SFTP 鎿嶄綔杩樿兘鐢級锛屼絾 `ssh_closed` 浜嬩欢鐓ф牱鍙戯紝閬僵鐓ф牱寮广€?
- **鍏抽敭璁ょ煡锛?* SSH **杩炴帴** 鈮?shell **閫氶亾**銆俽ussh 鍦ㄤ竴鏉?TCP 杩炴帴涓婂璺鐢ㄥ涓€氶亾锛汼FTP 瀛愮郴缁熸槸 `get_sftp_session` 鍦ㄥ悓涓€ `Handle` 涓婃柊寮€鐨勯€氶亾锛屽畬鍏ㄤ笉渚濊禆閭ｄ釜 shell 閫氶亾銆傜粰 SFTP 杩炴帴璇锋眰 shell 鏈氨娌℃湁鎰忎箟锛岃繕鎾炰笂 nologin 璐︽埛鐨勭珛鍗抽€€鍑恒€?
- **淇锛坄src-tauri/src/ssh.rs::connect`锛夛細** `conn_type == "sftp"` 鏃?*璺宠繃 PTY + shell 璇锋眰**鈥斺€斿彧 `channel_open_session`锛堥獙璇佽繛鎺ュ彲杈?+ 缁?reader 涓€涓€氶亾鐢ㄦ潵鎺㈡祴鐪熉稵CP 鏂紑锛夛紝涓嶉檮鍔犱换浣曠▼搴忋€係FTP 瀹為檯璧?`get_sftp_session` 鑷繁寮€瀛愮郴缁熼€氶亾銆傛棩蹇楀姞 `[ssh:xxx] SFTP session 鈥?skipping PTY/shell request`銆?
  - 淇濈暀 channel_reader 鐨?spawn锛歩dle select! 寰幆浠ｄ环鍙拷鐣ワ紝涓斿綋 TCP 鐪熸柇鏃?`channel.wait()` 杩斿洖 None/Close 鈫?鍙?`ssh_closed` 鈫?SftpPanel 姝ｇ‘鎻愮ず閲嶈繛锛堣繖鏄湡鏂繛锛岃鎻愮ず锛夈€俷ologin 璐︽埛鐨勩€宻hell 绔嬪嵆閫€鍑恒€嶈鎶ヨ鏍归櫎锛堜笉鍐嶈姹?shell 灏辨病鏈夐€€鍑猴級銆?
- **楠岃瘉锛?* `cargo check` PASS锛? warn锛?2.7s锛夈€傚緟 dev 瀹炴祴锛歋FTP 杩炴帴鎵撳紑鍚庝笉鍐嶅脊閲嶈繛閬僵銆佹枃浠跺垪琛ㄦ甯搞€?
- **褰卞搷闈細** 浠?`ssh::connect` 涓€澶勫垎鏀紱鏅€?SSH 缁堢杩炴帴璺緞瀹屽叏涓嶅彉锛坄is_sftp=false` 璧板師 PTY+shell 娴佺▼锛夈€係FTP 鐨?disconnect 璺緞涓嶅彉锛坮eader 鏀?Disconnect 鈫?close channel锛沝rop SshSession 鍏宠繛鎺ワ級銆?

### 闃舵 34锛歋FTP 鏂囦欢闈㈡澘瑙嗚涓?SSH 缁堢缁熶竴锛?026-06-22锛?
- **闇€姹傦細** SFTP 鏂囦欢鍒楄〃鐨勬枃浠跺す棰滆壊锛堝師鍏堟殩鐞ョ弨 `accent-secondary` 闈掔豢锛夈€佺被鍨?pill 寰界珷鐪嬬潃鍜?SSH 缁堢/渚ф爮椋庢牸涓嶇粺涓€锛屾暣浣撳亸銆屾潅銆嶃€傜敤鎴疯姹傛枃浠跺す棰滆壊鍜屽瓧浣撹窡 SSH 涓€鏍凤紝鏂囦欢淇℃伅灞曠ず涔熼噸鏂颁紭鍖栥€?
- **璁捐鏂瑰悜锛坔igh-end-visual-design 鎶€鑳芥寚寮曪級锛?* 閫?**Soft Structuralism** 璐ㄦ劅锛堜笌鐜版湁 Carbon 璁捐绯荤粺涓€鑷粹€斺€斿厠鍒剁殑涓€х伆 + 鍗曚竴 accent-primary 钃濅綔涓哄敮涓€寮鸿皟鑹诧紝閬垮厤澶氳壊鍣偣锛夈€傜粺涓€鎬т紭鍏堜簬鑺卞摠锛氱洰褰曡蛋 `accent-primary`锛堣摑锛夛紝涓?TerminalPanel/渚ф爮/TabBar 鐨?accent 璇箟瀹屽叏涓€鑷淬€?
- **鏀瑰姩锛坄src/components/SftpPanel.tsx` 鏂囦欢琛?+ 鍒楀ご锛夛細**
  - **鐩綍鍥炬爣**锛氫粠銆屾殩鐞ョ弨鏂瑰潡 + 鍙屾í绾裤€嶆敼涓烘爣鍑嗘枃浠跺す杞粨锛堝甫娣?accent-primary 濉厖锛夛紝鎻忚竟/濉厖閮界敤 `accent-primary` / `accent-primary-muted`鈥斺€斿拰 SSH 闈㈡澘鐨?accent 钃濆悓涓€濂楄涔夈€?
  - **鐩綍鍚嶉鑹?*锛歚accent-secondary`锛堥潚缁匡級鈫?`accent-primary`锛堣摑锛夛紝涓庡浘鏍囧悓鑹层€倃eight 淇濇寔 500銆?
  - **绫诲瀷鍒?*锛氬幓鎺夈€岃儗鏅?pill 寰界珷銆嶏紙`accent-secondary-muted` / `bg-surface-active` 搴曡壊 + 鍦嗚锛夆€斺€旇繖绉嶅鑹插窘绔犳槸瑙嗚鍣偣銆傛敼涓虹函鏂囨湰鏍囩锛堢洰褰?`accent-secondary` 鏂囧瓧銆佹枃浠?`text-tertiary`锛夛紝骞插噣銆佹妸瑙嗚鐒︾偣杩樼粰鏂囦欢鍚嶃€?
  - **鏂囦欢鍥炬爣**锛氱簿绠€涓哄崟椤垫枃妗ｈ疆寤擄紙鍘绘帀鍐呴儴鍙屾í绾跨粏鑺傦級锛宍text-tertiary` 鎻忚竟锛屽急鍖栧埌涓嶆姠鎴忋€?
  - **閫変腑鎬?*锛氱敤 `accent-primary-muted` 搴曡壊鏁磋楂樹寒锛堟浛浠ｄ粎 checkbox锛夛紝hover 涓?selected 浜掓枼锛坰elected 鏃?hover 涓嶈鐩栵級銆?
  - **闂磋窛/鑺傚**锛氬垪澶?琛?padding `5px 10px` 鈫?`5-6px 10px 5-6px 12px`锛堝乏渚у涓€鐐瑰懠鍚革級锛実ap `8` 鈫?`10`锛屽垪澶村瓧闂磋窛 `0.04em` 鈫?`0.06em`锛岃繃娓?`80ms ease` 鈫?`120ms cubic-bezier(0.4,0,0.2,1)`锛堜笌鍏ㄥ眬缂撳姩涓€鑷达級銆?
  - **鍘绘帀姣忚 `borderBottom`**锛氭秷闄ゅ瘑鎭愮殑妯嚎缃戞牸锛岄潬闂磋窛 + hover 鍖哄垎琛岋紙鏇寸幇浠ｏ級銆?
- **缁熶竴鎬у厬鐜扮偣锛?*
  - 鐩綍 accent 鑹茬幇鍦?= TerminalPanel/渚ф爮/鏍囩鏍忕殑 accent-primary 钃濓紙`CONN_COLOR.ssh`銆乣--accent-primary`锛夆€斺€擲FTP 鏍囩鍜?SSH 鏍囩鐨勩€屽彲浜や簰/瀵艰埅銆嶈瑙変俊鍙蜂竴鑷淬€?
  - 瀛椾綋锛氭枃浠跺悕鐢ㄥ簲鐢?body 瀛椾綋锛坄'Plus Jakarta Sans'` 鏍堬紝涓庡叏灞€涓€鑷达級锛屾棩鏈?鏉冮檺鐢?`'JetBrains Mono'` 绛夊鈥斺€斾笌 SSH 缁堢鐨勭瓑瀹芥覆鏌撳懠搴斻€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛? err锛夈€?
- **鍙栬垗锛?* 娌″姩宸ュ叿鏍?璺緞鏍?浼犺緭娴眰鈥斺€旈偅浜涘凡缁忓悎鐞嗭紝鏈鑱氱劍鐢ㄦ埛鎸囧嚭鐨勩€屾枃浠跺す棰滆壊 + 鏂囦欢淇℃伅灞曠ず銆嶃€傜被鍨嬪垪浠庡窘绔犻檷绾т负鏂囧瓧鏄湁鎰忛檷鍣紱鑻ュ悗缁鏇翠赴瀵屽彲鎸夋墿灞曞悕涓婅涔夎壊锛屼絾 YAGNI锛屽厛鍋氬厠鍒剁増銆?

### 闃舵 35锛欶TP 杩炴帴澶辫触銆屾棤鍚庡彴鏃ュ織 + 10060 瑁搁敊璇€嶏紙2026-06-22锛?
- **鐜拌薄锛?* FTP 杩炰笉涓婏紝鍓嶇鎶?`FTP connect failed: Connection error: ...杩炴帴灏濊瘯澶辫触 (os error 10060)`锛屼笖鍚庡彴鏃ュ織鏂囦欢閲?*瀹屽叏娌℃湁浠讳綍 ftp 鐩稿叧璁板綍**銆傜敤鎴烽棶 IP/绔彛鏄惁濉敊銆?
- **鏍瑰洜 1锛堟棤鏃ュ織锛夛細** `ftp_connect`锛坢ain.rs锛夊拰 `ftp::connect`锛坒tp.rs锛夊叏绋嬬敤 `eprintln!`/`Result?` 鈥斺€?浣?release 涓?stderr 琚?dup2 閲嶅畾鍚戝埌鏃ュ織鏂囦欢銆乨ebug 涓嬫墦鍒版帶鍒跺彴锛屼笖杩欎簺璺緞**鏍规湰娌℃墦浠讳綍鏃ュ織**锛坰sh_connect 鏈?`log::info!`/`log::error!`锛孎TP 瀵瑰簲浣嶇疆鏄┖鐨勶級銆傛墍浠ャ€岃繛涓嶄笂 + 娌℃棩蹇椼€嶄笉鏄?bug锛屾槸 FTP 璺緞鍘嬫牴娌℃帴鏃ュ織锛屼笌 SSH 涓嶅绉般€?
- **鏍瑰洜 2锛堣８ 10060锛夛細** `os error 10060` = Windows `WSAETIMEDOUT`锛孴CP 涓夋鎻℃墜鍦ㄩ粯璁よ秴鏃跺唴娌℃敹鍒?SYN-ACK銆?*鍑犱箮浠庢潵涓嶆槸瀵嗙爜閿欒**锛堝瘑鐮侀敊璇細璧板埌 `login()` 鎶?`FTP login failed`锛屼笉浼氬埌杩欎竴姝ワ級锛岃€屾槸缃戠粶鍙揪鎬э細鐩爣 IP/绔彛濉敊銆侀槻鐏鎷︽埅銆丗TP 鏈嶅姟娌¤窇銆佹垨鏈満鍒版湇鍔″櫒璺敱涓嶉€氥€備絾瑁搁敊璇瓧绗︿覆娌℃彁绀鸿繖鐐癸紝鐢ㄦ埛鍙兘鐬庣寽銆?
- **鏀瑰姩锛?*
  - `main.rs::ftp_connect`锛氬叆鍙ｅ姞 `log::info!("[ftp] connect requested: user@host:port (tls=, passive=, proxy=)")`锛岀粨鏋滃垎鏀?`log::info!`/`log::error!`锛堝绉?ssh_connect 鑼冨紡锛屼笌銆屾棩蹇楁寜澶╂粴鍔ㄦ枃浠躲€嶄竴鑷达級銆?
  - `ftp.rs::connect`锛氱洿杩炲垎鏀?`AsyncFtpStream::connect` 澶辫触鏃讹紝**妫€娴?10060/timeout 鍏抽敭瀛?*锛屾敼鍐欓敊璇负甯︿腑鏂囪瘖鏂殑鎻愮ず锛氥€孴CP 杩炴帴瓒呮椂锛坥s error 10060锛夈€傞€氬父涓嶆槸瀵嗙爜閿欒锛岃€屾槸锛氱洰鏍?host:port 鏃犳硶鍒拌揪鈥斺€旈槻鐏鎷︽埅銆両P/绔彛濉敊銆佹垨 FTP 鏈嶅姟鏈繍琛屻€傚厛 ping/缃戠粶楠岃瘉璇ュ湴鍧€绔彛鏄惁鍙揪銆嶃€備唬鐞嗗垎鏀繚鎸佸師鏍凤紙`connect_via_proxy` 宸叉湁 60s 瓒呮椂 + 涓枃鎻愮ず锛夈€傝繛鎺ュ悇闃舵锛坈onnecting/TCP established/session ready锛夐兘鎵?`log::info!`銆?
  - 鎶?`eprintln!` 鎹㈡垚 `log::info!`锛岃繖鏍?release 涔熻兘钀界洏鏃ュ織鏂囦欢锛堟寜澶╂粴鍔級銆?
- **楠岃瘉锛?* `cargo check` PASS锛? warning锛?2.8s锛夈€傞渶閲嶆柊 `cargo tauri dev` 鎵嶇敓鏁堛€?
- **缁欑敤鎴风殑鎺掓煡鎸囧紩锛?0060锛夛細**
  1. 纭 IP/绔彛鈥斺€擣TP 榛樿 21锛屼絾浣犺繖鍙扮敤鐨勯潪鏍囩鍙ｈ鏍稿疄锛沗ftp_tls` 蹇呴』鏄?`none`锛堝綋鍓嶇増鏈笉鏀寔 FTPS锛屽～ implicit/explicit 浼氱洿鎺ユ姤銆屾殏涓嶆敮鎸併€嶈€岄潪 10060锛夈€?
  2. 缃戠粶鍙揪鎬р€斺€斿湪鏈満鍛戒护琛?`Test-NetConnection <host> -Port <port>`锛圥owerShell锛夋垨 `telnet host port`锛岀湅 `TcpTestSucceeded` 鏄惁 True銆侳alse 灏辨槸闃茬伀澧?璺敱/鏈嶅姟娌¤捣銆?
  3. 鏄惁璧颁簡浠ｇ悊鈥斺€旇嫢 `proxy_type != none`锛屽厛鍏虫帀浠ｇ悊鐩磋繛璇曪紝鎺掗櫎鏄唬鐞嗙幆鑺傝秴鏃躲€?
  4. 琚姩妯″紡鈥斺€擿ftp_passive` 榛樿 true锛圢AT 鍙嬪ソ锛夛紝浣?10060 鍙戠敓鍦?*鎺у埗杩炴帴寤虹珛闃舵**锛屼笌琚姩/涓诲姩妯″紡鏃犲叧锛堥偅鏄暟鎹繛鎺ョ殑浜嬶級锛屾墍浠ヨ繖涓紑鍏充笉褰卞搷鏈鎶ラ敊銆?

### 闃舵 36锛氬惎鍔ㄧ櫧灞忔秷闄わ紙2026-06-23锛?
- **鐜拌薄锛?* 搴旂敤鍚姩銆佽烦鍒扮櫥褰曢〉涔嬪墠鏈変竴娈电櫧灞忚繃绋嬨€?
- **鏍瑰洜锛?* Tauri 绐楀彛榛樿 `visible: true`锛學ebView 鍦?React 鎸傝浇棣栧抚涔嬪墠灏辨妸绐楀彛鏄剧ず鍑烘潵锛沗index.html` 娌℃湁浠讳綍鍐呰仈鏍峰紡锛岀櫧搴曡８ `#root` 琚敾鍑烘潵 鈫?鐧介棯銆侰SS 鐢ㄤ簡 `@import "../assets/iconfont/iconfont.css"`锛屽湪 import 瀹屾垚鍓?body 鏃犺儗鏅壊銆?
- **鏂规锛堝交搴曟秷闄わ級锛?* 绐楀彛鍏堥殣钘忥紝绛夊墠绔甯х湡姝ｇ粯鍒跺畬鍐嶆樉绀恒€?
  1. `tauri.conf.json` 涓荤獥鍙ｅ姞 `"visible": false`銆?
  2. `main.rs::setup` 閲?`app.listen_any("dom-ready", 鈥?` 鈫?鏀跺埌浜嬩欢鍚?`window.show()` + `set_focus()`锛涘彟璧蜂竴涓?4s `tokio::time::sleep` 瀹夊叏缃戯紝鍓嶇鑻ユ病鍙戜簨浠朵篃寮哄埗鏄剧ず锛堥槻姝?JS 鎶ラ敊鎶婄獥鍙ｅ崱鎴愭案涔呬笉鍙锛夈€傞渶 `use tauri::Listener;` 鎵嶈兘鐢?`listen_any`銆?
  3. `src/main.tsx` 鍦?`ReactDOM.render` 涔嬪悗锛岀瓑涓や釜 `requestAnimationFrame`锛堢‘淇濇祻瑙堝櫒宸叉彁浜ょ粯鍒惰€岄潪浠呮帓闃?React 宸ヤ綔锛夛紝鍐?`emit("dom-ready")`锛涚敤鍔ㄦ€?`import("@tauri-apps/api/event")` + try/catch锛屼繚璇佸湪绾祻瑙堝櫒锛坄npm run dev` 鐩存帴寮€缃戦〉锛変笅涔熶笉鎶ラ敊銆?
  4. `index.html` 鍐呰仈鍏滃簳锛歚html/body` 鑳屾櫙鍐欐 `#0d1117`锛? 娣辫壊涓婚 `--bg-base`锛夛紝`#root:empty::after` 鐢讳竴涓?`--accent-primary` 鑹茬殑鏃嬭浆 spinner鈥斺€斿嵆渚跨獥鍙ｅ凡鏄剧ず浣?React 杩樻病鎸傝浇锛堟瘮濡傛參鏈哄櫒/瀹夊叏缃戣Е鍙戯級锛岀湅鍒扮殑涔熸槸娣辫壊 + 鍔犺浇鍦堬紝鑰岄潪鐧藉睆銆俁eact 涓€鎸傝浇 `#root` 涓嶅啀 `:empty`锛宻pinner 鑷姩娑堝け銆?
- **淇敼鐨勬枃浠讹細**
  - `index.html` 鈥?鍐呰仈 boot splash 鏍峰紡锛堟繁鑹茶儗鏅?+ `#root:empty` spinner锛?
  - `src-tauri/tauri.conf.json` 鈥?涓荤獥鍙?`"visible": false`
  - `src-tauri/src/main.rs` 鈥?`use tauri::Listener`锛泂etup 閲屾敞鍐?`dom-ready` 鐩戝惉 + 4s 寮哄埗鏄剧ず瀹夊叏缃?
  - `src/main.tsx` 鈥?鍙?rAF 鍚?`emit("dom-ready")`
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛? warning锛夈€傞渶 `cargo tauri dev` / `cargo tauri build` 鎵嶈兘鐪嬪埌鏁堟灉锛堟敼浜?Rust锛夈€?
- **鏉冭　/娉ㄦ剰锛?*
  - 瀹夊叏缃戠殑 4s 鏄?*鏈€鍧忓厹搴?*锛屾甯歌矾寰勫墠绔嚑鍗佹绉掑氨 emit 浜嗭紝绐楀彛鍑犱箮鏄嵆鏃跺嚭鐜扳€斺€斾笉浼氱湡鐨勭瓑鍒?4s銆?
  - 鐢?`listen_any`锛坅ny-target锛夎€岄潪 `listen`锛岄伩鍏?sender label 鍖归厤闂锛涗簨浠跺悕 `dom-ready` 鏄嚜瀹氫箟鐨勶紝涓嶅拰 Tauri 鍐呯疆鍐茬獊銆?
  - 娴呰壊涓婚鐢ㄦ埛锛氬厹搴曡儗鏅槸娣辫壊 `#0d1117`锛屼粎瀛樺湪浜庛€岀獥鍙ｅ凡鏄剧ず浣?React 鏈寕杞姐€嶇殑鏋佺煭鐬棿锛汻eact 鎸傝浇鍚?`ColorSchemeProvider` 绔嬪嵆瑕嗙洊涓烘祬鑹层€傚彲鎺ュ彈锛堣繖鐬棿鑲夌溂鍩烘湰涓嶅彲瑙侊級銆?
  - 鑻ュ皢鏉ュ姞澶氱獥鍙ｏ紝姣忎釜鏂扮獥鍙ｈ澶嶅埢鍚屾牱鐨?visible:false + dom-ready 妯″紡锛屽惁鍒欐柊绐楀彛浼氱櫧闂€?

### 闃舵 37锛氶暱鎸夋嫋鍔ㄨ繛鎺ュ埌鏂囦欢澶圭Щ鍔紙2026-06-23锛?
- **鐜拌薄/闇€姹傦細** 杩炴帴绠＄悊鐩墠鍙兘杩涚紪杈戝璇濇鏀?`group_path` 鏉ョЩ鍔ㄨ繛鎺ワ紝澶閲嶃€傜敤鎴疯锛?*闀挎寜杩炴帴鎷栧埌鏂囦欢澶逛笂鏉炬墜鍗崇Щ鍔?*锛屽苟闃叉璇搷浣滐紱涓旀嫋鍔ㄦ椂甯屾湜**鍙睍绀烘枃浠跺す**锛堥殣钘忔枃浠跺す涓嬬殑杩炴帴锛夛紝閬垮厤鏂囦欢澶归噷杩炴帴澶銆佽涓€鐩翠笅婊戞墠鑳芥嫋鍒扮洰鏍囨枃浠跺す銆?
- **鏂规锛?*
  1. **鍚庣鏂板涓撶敤鍗曞垪鏇存柊鍛戒护** `move_connection(conn_id, new_group_path)`锛歚UPDATE connections SET group_path=?1 WHERE id=?2`锛屽弬鏁板寲闃叉敞鍏ワ紝**鍙敼涓€鍒?*锛屼笉纰?keyring銆佷笉閲嶆柊鍔犲瘑 host/user/key锛堜笉鍍?`save_connection` 璧?`INSERT OR REPLACE` + secret 閲嶅鐞嗭紝绉诲姩鏄函鏂囦欢澶归噸鍒嗛厤锛屼笓鐢ㄥ懡浠ゆ渶骞插噣瀹夊叏锛夈€俙new_group_path` 缁?`normalize_folder_path` 褰掍竴锛坄/` = 鍙栨秷褰掔被锛夈€俤b.rs + main.rs + generate_handler 娉ㄥ唽銆?
  2. **鍓嶇 API** `moveConnection(connId, newGroupPath)`锛坅pi.ts锛宨nvoke 鍙傛暟 camelCase 鈫?Rust snake锛夈€?
  3. **浜や簰鏍稿績** `useConnectionDrag`锛堟柊鏂囦欢 `src/hooks/`锛夛細pointer 浜嬩欢 + `setPointerCapture`锛?*闀挎寜 600ms**锛堢Щ鍔ㄧ宸ヤ笟鏍囧噯锛涚敤鎴峰師鎻?2s锛屾潈琛″悗鏀?600ms 鏇磋窡鎵嬶紝浠嶅疄鐜颁负鍙皟甯搁噺锛? **5px 绉诲姩鍙栨秷闃堝€?*锛堟櫘閫氱偣鍑?鍙屽嚮缁濅笉浼氳瑙﹀彂锛夈€備袱闃舵锛?
     - Phase A锛?鈫?00ms锛岀洃鍚湪鎹曡幏鍏冪礌涓婏級锛氳秴鏃?鈫?杩涘叆 B锛沺ointermove>5px / pointerup / pointercancel / window blur / ESC 鈫?娓呯悊鍙栨秷銆?
     - Phase B锛堟嫋鎷戒腑锛岀洃鍚湪 document锛夛細hit-test `document.elementFromPoint(x,y)?.closest('[data-folder-path]')`锛宍hoverFolderPath` 鍙樺寲鎵?setState锛堥槻姣忓抚閲嶆覆锛夛紱pointerup 鈫?鐩爣闈炵┖涓斺墵褰撳墠鏂囦欢澶瑰垯 `await moveConnection` + 鑷姩灞曞紑 + `onRefresh`锛屽惁鍒欐棤鎿嶄綔锛汦SC / pointercancel / blur 鈫?鍙栨秷銆俙isMovingRef` 闃茶繛鐐圭珵鎬併€?
  4. **Sidebar 鎺ョ嚎**锛歚walk` 缁?FolderRow 浼?`isDropTarget` + 鏍?div 鍔?`data-folder-path`锛岀粰 ConnRow 浼?`draggingConnId` + `onPointerDown={beginDrag}`銆?*鎷栨嫿婵€娲绘椂锛坄isDragging`锛夊垪琛ㄥ垏鎹负銆屽彧鏂囦欢澶广€嶈鍥?*鈥斺€擿walk` 閲?`isDragging` 鏃跺己鍒?`isOpen=true` 涓斿彧閫掑綊 `children`銆佽烦杩?`conns`锛岃繖鏍风洰鏍囨枃浠跺す绔嬪嵆鍙鏃犻渶涓嬫粦锛涙澗鎵嬫仮澶嶅師瑙嗗浘銆侳olderRow/ConnRow 鐢?`React.memo` 鍖呰９锛堟嫋鎷?hover 鍙樺寲鍙噸娓叉簮琛?鐩爣琛岋級銆傛悳绱㈢粨鏋滆鍥句笉鎺?`beginDrag`锛坉epth=0 鏃犳枃浠跺す锛屾嫋鎷戒笉杩炶疮锛夈€俙onContext` 鍦ㄦ嫋鎷戒腑鏃╅€€銆?
  5. **瑙嗚**锛氳鎷栬 `opacity:0.4`+`shadow-glow`+`scale(1.02)`+**`pointerEvents:none`**锛堝叧閿紝鍚﹀垯 elementFromPoint 鍛戒腑鑷繁锛夛紱鐩爣鏂囦欢澶?`--accent-primary-muted` bg + `inset border-accent`锛沚ody `cursor:grabbing`+`userSelect:none`銆備笉鍋氳窡闅忓厜鏍囩殑 ghost锛坴1锛夈€?
- **淇敼鐨勬枃浠讹細**
  - `src-tauri/src/db.rs` 鈥?`move_connection`锛堝弬鏁板寲鍗曞垪 UPDATE锛?
  - `src-tauri/src/main.rs` 鈥?`move_connection` 鍛戒护 + 娉ㄥ唽 `generate_handler!`
  - `src/api.ts` 鈥?`moveConnection`
  - `src/hooks/useConnectionDrag.ts` 鈥?**鏂版枃浠?*
  - `src/components/Sidebar.tsx` 鈥?鎺ョ嚎銆乵emo銆乨ata-folder-path銆佹嫋鎷芥椂鍙覆鏌撴枃浠跺す銆佽瑙夊垎鏀€佽嚜鍔ㄥ睍寮€銆乷nContext 鏃╅€€
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛? warning锛夈€傞渶 `cargo tauri dev` 鎵嬫祴鍏ㄤ氦浜掋€?
- **琛ュ厖锛氭嫋鍔ㄦ彁绀烘í骞咃紙2026-06-23锛?*
  - **鐜拌薄锛?* 鎷栧姩娌℃湁浠讳綍鎻愮ず锛岀敤鎴蜂笉鐭ラ亾杩欎釜鎵嬪娍鐨勪綔鐢ㄣ€佷篃涓嶇‘瀹氬綋鍓嶄細钀藉埌鍝釜鏂囦欢澶广€?
  - **鏀瑰姩锛?* `Sidebar.tsx` 鏂板 `DragHint` 缁勪欢锛宍isDragging` 鏃跺湪鍒楄〃椤堕儴娓叉煋妯箙锛歛ccent 鑹茶儗鏅?+ border-accent锛屼笁琛屸€斺€斻€屾鍦ㄧЩ鍔ㄣ€岃繛鎺ュ悕銆嶃€嶏紙璇存槑浣滅敤锛?銆屾嫋鍒版枃浠跺す涓婃澗寮€鍗冲彲绉诲姩鈥SC 鎴栫┖鐧藉鍙栨秷銆嶏紙璇存槑鎿嶄綔锛?銆屽綋鍓嶇洰鏍囷細馃搧 鏂囦欢澶瑰悕銆嶏紙鎴栥€岀Щ鍒版枃浠跺す涓婁互閫夋嫨銆嶅綋鎮仠绌虹櫧锛夈€傜洰鏍囬殢 `dragState.hoverFolderPath` 瀹炴椂鏇存柊锛坔it-test 宸叉湁鐨勭姸鎬侊級銆傚鐢ㄧ幇鏈?`fadeIn` 鍔ㄧ敾銆?
  - **楠岃瘉锛?* `npx tsc --noEmit` PASS銆?
- **宸茬煡闄愬埗锛?* 鎷栧埌鍒楄〃涓婁笅杈圭紭涓嶈嚜鍔ㄦ粴鍔紙`listContainer` overflowY:auto锛寁1 涓嶅仛锛夛紱鎷栨嫿涓?ConnRow 鐨?鈰?鑿滃崟鑻ュ凡寮€浼氫繚鎸侊紱600ms 甯搁噺鍙皟銆?
- **鍏抽敭鍧戯紙宸茶閬匡級锛?* 琚嫋琛屽繀椤?`pointerEvents:none`锛屽惁鍒?`elementFromPoint` 鍛戒腑琚嫋琛岃嚜韬€岄潪涓嬪眰鏂囦欢澶癸紙鏈ā寮忔渶甯歌 bug锛夛紱hit-test setState 蹇呴』 `!==` 瀹堝崼闃叉瘡甯ч噸娓叉暣琛紱Phase A鈫払 鍒囨崲鏃?`dragActivated` 鏍囧織 + 涓ゆ releaseCapture 闃叉閲嶅閲婃斁/娉勬紡 capture銆?

## 浜旈棶閲嶅惎妫€鏌?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 37 complete锛堥暱鎸夋嫋杩炴帴鍒版枃浠跺す绉诲姩锛氬悗绔?move_connection 鍗曞垪鏇存柊 + 鍓嶇 useConnectionDrag 涓ら樁娈?+ Sidebar 鎷栨嫿鏃跺彧鏄剧ず鏂囦欢澶癸紱tsc/cargo check 鍙岀豢锛夛紝寰?dev 鎵嬫祴鍏ㄤ氦浜?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴锛氶暱鎸?600ms 鎷栧姩銆佹嫋鏃跺彧鏄剧ず鏂囦欢澶广€佹澗鎵嬬Щ鍔?鑷姩灞曞紑銆丒SC/绌虹櫧鍖?鍚屾枃浠跺す 鏃犳搷浣溿€佺偣鍑?鍙屽嚮鏈璇激 |
| 鐩爣鏄粈涔堬紵 | 闀挎寜杩炴帴鎷栧埌鏂囦欢澶瑰嵆绉诲姩锛岄槻璇搷浣滐紙600ms+5px 闃堝€?ESC锛夛紱鎷栨嫿涓彧灞曠ず鏂囦欢澶规棤闇€涓嬫粦鎵剧洰鏍囷紱閲嶅惎鍚庤繛鎺ユ寔涔呭湪鐩爣鏂囦欢澶?|
| 鎴戝鍒颁簡浠€涔堬紵 | 绉诲姩杩炴帴搴斿仛涓撶敤鍗曞垪 UPDATE锛堜笉纰?keyring/涓嶉噸鍔犲瘑锛夛紝姣斿鐢?save_connection 鐨?INSERT OR REPLACE 骞插噣瀹夊叏锛涢暱鎸夋嫋鎷界敤 pointer+setPointerCapture 涓ら樁娈碉紙A 鍏冪礌鎹曡幏妫€娴嬮暱鎸夈€丅 绉诲埌 document hit-test锛夛紱琚嫋琛屽繀椤?pointerEvents:none 鍚﹀垯 elementFromPoint 鍛戒腑鑷韩锛堟渶甯歌 bug锛夛紱hit-test setState 蹇呴』 !== 瀹堝崼闃叉瘡甯ч噸娓叉暣琛紱鐢ㄦ埛鐥涚偣銆屾枃浠跺す杩炴帴澶氳涓嬫粦鎵剧洰鏍囥€嶇敤銆屾嫋鎷芥椂鍙覆鏌撴枃浠跺す銆嶄紭闆呰В鍐筹紙walk 閲?isDragging 鍒嗘敮寮哄埗灞曞紑+璺宠繃 conns锛?|
| 鎴戝仛浜嗕粈涔堬紵 | db.rs+main.rs move_connection 鍙傛暟鍖栧崟鍒楁洿鏂板苟娉ㄥ唽锛沘pi.ts moveConnection锛涙柊 useConnectionDrag hook锛?00ms/5px/涓ら樁娈?hit-test/in-flight 瀹堝崼锛夛紱Sidebar 鎺ョ嚎+memo+data-folder-path+鎷栨嫿鍙樉绀烘枃浠跺す+瑙嗚+鑷姩灞曞紑+onContext 鏃╅€€锛泃sc+cargo check 鍙岀豢锛沺rogress 鍚屾 |

### 闃舵 38锛氳繛鎺ユ祴璇曟寜閽?+ AI 閰嶇疆娴嬭瘯鎸夐挳 + AI 杈撳叆妗嗗姞楂橈紙2026-06-23锛?
- **闇€姹傦紙鐢ㄦ埛鍘熻瘽涓夋潯锛夛細**
  1. 鏂板缓/缂栬緫杩炴帴瀵硅瘽妗嗗鍔犮€屾祴璇曘€嶆寜閽€斺€斾繚瀛樺墠鍏堥獙璇?ssh/sftp/ftp/local 鑳藉惁杩為€氥€?
  2. 璁剧疆 鈫?AI 鍔╂墜锛屻€屼繚瀛?AI 閰嶇疆銆嶆梺鍔犮€屾祴璇曘€嶆寜閽€斺€旈獙璇佸綋鍓嶅～鍐欑殑 provider/key/model/baseUrl 鐪熻兘鐢紝涓嶅繀鍏堜繚瀛樸€?
  3. 涓婚〉 AI 鍔╂墜杈撳叆妗嗗姞楂樷€斺€旂幇鍦?`rows=2`锛岄殢渚垮嚑涓瓧灏辨孩鍑恒€?
- **鍏抽敭璁捐鍐崇瓥锛?*
  - **杩炴帴娴嬭瘯鎶藉叕鍏辨嫧鍙?璁よ瘉娈点€?* `ssh::connect` 鍘熸湰鎶娿€屾嫧鍙?璁よ瘉銆嶄笌銆屽紑 channel/PTY/shell/璧?reader/鎻?session銆嶆弶鍦ㄤ竴璧枫€傛娊鍑?`dial_and_authenticate(state, config, open_channel) -> Result<Handle, String>`锛堣繑鍥炲凡璁よ瘉 Handle锛屼笉娉ㄥ唽 session銆佷笉璧?reader銆佷笉璇锋眰 PTY/shell銆佷笉鍙戜簨浠讹級锛宍connect` 涓庢柊澧炵殑 `ssh::test_connection` 閮借皟瀹冣€斺€?*鐪熷疄杩炴帴涓庢祴璇曡蛋鍚屼竴鏉¤璇?host-key TOFU 璺緞**锛屾祴璇曡兘鎶撳埌鐪熷疄杩炴帴鍚屾牱鐨勫け璐ャ€俙open_channel=true` 鏃跺紑涓€涓?session channel 绔嬪嵆涓㈠純锛屾牎楠屻€岃璇侀€氳繃浣嗘湇鍔″櫒鎷掑紑 channel銆嶏紙鐪熷疄 SFTP 渚濊禆鐨勫鐢ㄨ矾寰勶級銆?
  - **AI 娴嬭瘯鐢ㄩ潪娴佸紡鏈€灏忚姹傘€?* 澶嶇敤 `load_settings`锛坴ault 閰嶇疆+瑙ｅ瘑 key锛? `Provider::{endpoint, auth_headers, build_body}` + `truncate`銆俙build_body` 榛樿 `stream:true`锛屾祴璇曢噷 `as_object_mut().insert("stream", false)` 瑕嗙洊 + `max_tokens=16`銆俻rompt 鐢?`"ping"` + 绯荤粺鎻愮ず銆孯eply with the single word: ok銆嶃€?*闈炴祦寮忚€岄潪娴佸紡娑堣垂棣?token**锛氬崟娆?HTTP 寰€杩旓紝success/fail 璇箟骞插噣锛屼笖**缁濅笉 emit `ai_token`/`ai_done`**锛屼笉浼氬共鎵版墦寮€鐨?AiPanel 娴佸紡璁㈤槄銆?00 鍗宠涓鸿璇?绔偣閫氳繃锛沗extract_reply_snippet` 灏藉姏鎶戒竴琛屽洖澶嶅噾杩涙垚鍔熸秷鎭紝瑙ｆ瀽澶辫触灏遍檷绾с€?
  - **AI 娴嬭瘯涓嶈嚜鍔ㄤ繚瀛樸€?* 璧?overrides锛氳〃鍗曞疄鏃跺€硷紙鍚湭淇濆瓨鐨?key锛夌洿浼?`ai_test_settings`锛岀┖涓?undefined 鍥為€€ vault 鍊硷紙涓庝繚瀛樼殑銆岀┖ key=淇濇寔涓嶅彉銆嶅悓璇箟锛夈€傜敤鎴峰彲鍙嶅璇?model/baseUrl/key 涓嶈惤搴撱€?
  - **AI 杈撳叆妗嗙敤 `rows=4` + `minHeight:96` + `lineHeight:1.5`锛屼笉鍋?auto-grow銆?* auto-grow 瑕佹祴 scrollHeight/澶勭悊 placeholder/shrink锛屽緱涓嶅伩澶憋紱鍥哄畾 4 琛?+ minHeight 鍗宠В鍐炽€屽嚑涓瓧婧㈠嚭銆嶃€?
- **鏀瑰姩锛堝悗绔級锛?*
  - `ssh.rs` 鈥?鏂板 `dial_and_authenticate`锛堜粠 `connect` 鎶藉嚭鎷ㄥ彿+璁よ瘉锛? `test_connection`锛坉ial+auth+寮€ channel+`handle.disconnect(Disconnect::ByApplication, ...)`锛岃繑鍥炪€岃繛鎺ユ垚鍔燂紙N ms锛岃璇佹柟寮?瀵嗙爜/绉侀挜锛夈€嶏級锛沗connect` 鏀硅皟 helper锛沗use russh::Disconnect`銆?
  - `ftp.rs` 鈥?`test_connection`锛坄connect`+`disconnect` QUIT锛岃繑鍥炪€岃繛鎺ユ垚鍔燂紙N ms锛孎TP 鐧诲綍閫氳繃锛夈€嶏級锛汧TPS 浼氳 `connect` 鐩存帴鎷掋€?
  - `local.rs` 鈥?`test_connection`锛坄spawn_blocking` 閲?`native_pty_system().openpty`+`spawn_command`+绔嬪嵆 `child.kill()`+`wait`锛岃繑鍥炪€岃繛鎺ユ垚鍔燂紙N ms锛宻hell 鍙惎鍔級銆嶏紱鎶撳潖璺緞/缂哄彲鎵ц鏂囦欢锛屼笉璧?reader銆佷笉鍏?session map锛夈€?
  - `ai.rs` 鈥?`AiTestOverrides` 缁撴瀯 + `test_settings`锛坥verride鈫抈load_settings`鈫抈build_body`+`stream:false`+`max_tokens:16`鈫抮eqwest+鍙€?proxy鈫?00 鍙?snippet / 闈?200 鎴柇鎶ラ敊锛? `extract_reply_snippet`銆?
  - `main.rs` 鈥?`test_connection` 鍛戒护锛堝鐢?`ssh_connect`/`ftp_connect` 鐨?keyring 瑙ｆ瀽锛氳〃鍗曞瘑鐮佷紭鍏堛€佺己鍒欐寜 `config.id` 鍙?keyring锛沺assword-auth 鏂板缓鏃?id 涓旀棤瀵嗙爜鏃舵嫆缁濇祴璇曢槻绌哄瘑鐮佽Е鍙戦攣鍙凤紱鏁翠釜鎺㈤拡濂?15s `tokio::time::timeout`锛? `ai_test_settings` 鍛戒护锛涗袱鍛戒护娉ㄥ唽杩?`generate_handler!`銆?
- **鏀瑰姩锛堝墠绔級锛?*
  - `api.ts` 鈥?`testConnection(config)`銆乣AiTestOverrides` 鎺ュ彛銆乣aiTestSettings(overrides?)`銆?
  - `ConnectionDialog.tsx` 鈥?鎶?`buildConfig(): ConnectionConfig | null`锛堟妸 `handleSave` 鐨勬牎楠?閰嶇疆缁勮鎼嚭锛宍handleSave`/`handleTest` 鍏辩敤锛屼繚璇併€屾祴璇曠殑灏辨槸瑕佷繚瀛樼殑銆嶏級锛涙柊澧?`testing`/`testResult` 鐘舵€?+ `handleTest`锛沠ooter 鏀?`flexDirection:column`鈥斺€斾笂灞傛斁缁撴灉妯箙锛堢豢/绾?+ 鉁?鉁?+ `word-break`锛屽鐢?`--success/--error` + `--success-muted/--error-muted`锛夛紝涓嬪眰鎸夐挳琛屽湪銆屽彇娑堛€嶃€屼繚瀛樸€嶉棿鎻掋€屾祴璇曘€嶆寜閽紙`marginRight:auto` 闈犲乏銆乬host 浜岀骇鏍峰紡銆乣disabled=testing||saving`锛夈€俵ocal 涓?remote 鍏辩敤鍚?footer锛坄buildConfig` 宸叉寜 connType 鍒嗘敮锛夈€?
  - `SettingsPanel.tsx` 鈥?`aiTesting` 鐘舵€?+ `handleTestAi`锛堣〃鍗曞€间綔 overrides 浼?`aiTestSettings`锛屽鐢?`aiMsg` 鏄剧ず锛?s 娓呴櫎锛夛紱銆屼繚瀛?AI 閰嶇疆銆嶅墠鎻掋€屾祴璇曘€嶆寜閽紙ghost 浜岀骇锛屼袱鎸夐挳浜?disabled锛夛紱import `aiTestSettings`銆?
  - `AiPanel.tsx` 鈥?textarea `rows={2}鈫抺4}`锛宍textareaStyle` 鍔?`lineHeight:1.5` + `minHeight:96`锛涗笉鍔?`btnStyle`锛坔eight:38锛変笌 `alignItems:flex-end`锛屽彂閫侀敭鑷劧璐村簳銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛? warning锛屼慨浜?`connect` 閲?`let mut handle` 鈫?`let handle` 鐨勬湭鐢?mut锛夈€傞渶 `cargo tauri dev` 鎵嬫祴涓夋潯浜や簰銆?
- **杈圭晫/閿欒澶勭悊瑕佺偣锛?*
  - 杩炴帴娴嬭瘯锛氱┖瀵嗙爜锛堟柊寤烘棤 id锛夊厛浜庣綉缁滆繑鍥炪€屾湭濉啓瀵嗙爜锛屾棤娉曟祴璇曘€嶏紙闃茬┖瀵嗙爜琚澶辫触娆℃暟鈫掗攣鍙凤級锛涚紪杈戞ā寮忔棤琛ㄥ崟瀵嗙爜鈫掓寜 id 鍙?keyring锛沄ault 閿佸畾鈫抈require_dek` 鎶ラ敊锛汧TPS鈫抈connect` 鐩存嫆銆屾殏涓嶆敮鎸併€嶅師鏍烽€忓嚭锛涗唬鐞嗘彙鎵嬪凡鏈?10s 瓒呮椂 + 澶栧眰 15s 鍏滃簳锛涗换浣曞垎鏀兘涓嶅叆 session map锛圫SH `disconnect` / FTP QUIT / local `kill+wait`锛夛紱known_hosts TOFU 鐓у父锛圡ITM 涓绘満閿笉鍖归厤鈫掓祴璇曞け璐ワ級銆?
  - AI 娴嬭瘯锛氭湭淇濆瓨 key锛堣〃鍗曟湁锛夆啋 override 鐩寸敤锛沰ey 绌?vault 鏈夆啋鐢?vault锛沰ey 绌?vault 绌衡啋`load_settings` 鎶ャ€屾湭閰嶇疆 API key銆嶏紱vault 閿佲啋鎶ャ€孷ault 鏈В閿併€嶏紱鏈煡 provider鈫抈Provider::parse` 鎶ラ敊锛涘潖浠ｇ悊 URL鈫掓姤銆屼唬鐞嗛厤缃棤鏁堛€嶏紱4xx/5xx鈫掋€孉I 鎺ュ彛杩斿洖 {status}锛歿鎴柇 body}銆嶏紙鍏稿瀷 401 鍧?key / 404 鍧?baseUrl / 429 闄愭祦锛夛紱200 浣?body 涓嶅彲瑙ｆ瀽鈫抯nippet 绌猴紝鎴愬姛娑堟伅闄嶇骇锛?*鏃犱簨浠舵硠婕?*銆?
  - 杈撳叆妗嗭細Enter 浠嶅彂閫併€丼hift+Enter 鎹㈣锛堢幇鍦ㄥ琛屾洿鑸掗€傦級锛沺laceholder 鍦?4 琛屽唴姝ｅ父鎹㈣锛涘贰妫€鎸夐挳/澶撮儴涓嶅彈褰卞搷銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 38锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 38 complete锛堣繛鎺ユ祴璇曟寜閽?ssh/sftp/ftp/local + AI 閰嶇疆娴嬭瘯鎸夐挳 + AI 杈撳叆妗嗗姞楂橈紱鍚庣 dial_and_authenticate 鎶藉彇 + test_connection/ai::test_settings + 涓ゅ懡浠ゆ敞鍐岋紱鍓嶇 api.ts 鍖呰 + ConnectionDialog buildConfig 鎶藉彇 + SettingsPanel/AiPanel 鏀瑰姩锛泃sc/cargo check 鍙岀豢锛夛紝寰?dev 鎵嬫祴涓夋潯浜や簰 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴锛氣憼鏂板缓 SSH 濉〃鈫掓祴璇曗啋缁裤€岃繛鎺ユ垚鍔?N ms銆? 閿欏瘑鐮佲啋绾?Authentication failed锛涚紪杈戝凡鏈夛紙瀵嗙爜鐣欑┖锛夆啋娴嬭瘯鈫掑悗绔彇 keyring 鎴愬姛锛泂ftp/ftp/local 鍚勫垎鏀紱鈶I 鍋?model鈫掔孩 4xx銆佹敼鍥炵豢銆佸～鏈夋晥鏈繚瀛?key鈫掔豢锛堥獙璇?override锛夛紱鈶iPanel 杈撳嚑涓瓧涓嶆孩鍑恒€佸琛岀矘璐存槑鏄惧彉楂?|
| 鐩爣鏄粈涔堬紵 | 淇濆瓨鍓嶉獙璇佽繛鎺?AI 閰嶇疆鍙敤锛堥伩鍏嶅弽澶嶄繚瀛?杩炴帴-鏀?鍐嶄繚瀛橈級锛汚I 杈撳叆妗嗕笉鍐嶅嚑涓瓧灏辨孩鍑?|
| 鎴戝鍒颁簡浠€涔堬紵 | 杩炴帴娴嬭瘯搴旀娊鍏叡鎷ㄥ彿+璁よ瘉娈碉紙dial_and_authenticate锛夎鐪熷疄杩炴帴涓庢祴璇曞悓璺緞銆佸悓 host-key TOFU 淇濊瘉锛涜繛鎺ユ祴璇曞繀椤诲鐢?ssh_connect/ftp_connect 鐨?keyring 瑙ｆ瀽 + 闃茬┖瀵嗙爜閿佸彿 + 澶栧眰瓒呮椂鍏滃簳锛汚I 娴嬭瘯鐢ㄩ潪娴佸紡鏈€灏忚姹傦紙stream:false+max_tokens:16锛夎€岄潪娑堣垂棣?token鈥斺€斿共鍑€涓旂粷涓?emit 浜嬩欢姹℃煋 AiPanel锛汚I 娴嬭瘯璧?overrides 涓嶈嚜鍔ㄤ繚瀛橈紝绗﹀悎銆岃瘯浜嗗啀鍐冲畾銆嶅績鏅猴紱textarea 鍔犻珮鐢?rows+minHeight 鑰岄潪 auto-grow锛堢畝鍗曚笖瑙ｅ喅鐥涚偣锛夛紱ConnectionDialog 鎶?buildConfig 璁?save/test 鍏辩敤閰嶇疆缁勮闃叉紓绉?|
| 鎴戝仛浜嗕粈涔堬紵 | ssh.rs 鎶?dial_and_authenticate + test_connection + use Disconnect銆乧onnect 鏀硅皟 helper锛堜慨鏈敤 mut锛夛紱ftp.rs test_connection锛沴ocal.rs test_connection锛坰pawn+kill+wait锛夛紱ai.rs AiTestOverrides+test_settings+extract_reply_snippet锛沵ain.rs test_connection锛坘eyring 瑙ｆ瀽+闃茬┖瀵嗙爜+15s 瓒呮椂锛? ai_test_settings 鍛戒护 + 涓ゅ generate_handler 娉ㄥ唽锛沘pi.ts testConnection/AiTestOverrides/aiTestSettings锛汣onnectionDialog 鎶?buildConfig+testing/testResult 鐘舵€?handleTest+footer 鍔犳祴璇曟寜閽笌缁撴灉妯箙锛汼ettingsPanel aiTesting+handleTestAi+娴嬭瘯鎸夐挳+import锛汚iPanel rows=4+textareaStyle 鍔?lineHeight/minHeight锛泃sc+cargo check 鍙岀豢锛沺rogress+README 鍚屾 |

### 闃舵 39锛氱増鏈彿灞曠ず + 鑷姩妫€娴嬫洿鏂?+ 棣栧惎鏇存柊鍐呭 + 瀛椾綋瀛楁绉诲埌搴曢儴锛?026-06-24锛?
- **闇€姹傦紙鐢ㄦ埛鍘熻瘽鍥涙潯锛夛細**
  1. 鐣岄潰娣诲姞鐗堟湰鍙蜂俊鎭€?
  2. 璁捐鑷姩妫€娴嬫洿鏂板拰鏇存柊鎻愮ず鍔熻兘銆?
  3. 鐗堟湰鏇存柊鍚庨娆″睍绀烘洿鏂板唴瀹广€?
  4. 鏂板缓杩炴帴锛屽瓧浣撻€夋嫨鏀惧湪鏈€涓嬮潰锛屼笉瑕佹斁鍦ㄤ笂闈€?
- **鍏抽敭璁捐鍐崇瓥锛?*
  - **鏇存柊鏈哄埗 = 杞婚噺妫€娴?鎻愮ず锛屼笉闆嗘垚 tauri-plugin-updater銆?* 涓嶅紩绛惧悕瀵嗛挜/manifest/CI 浜х墿锛屽鍚?Gitee 鎵樼锛汻ust 鐢ㄥ凡鏈?`reqwest` 鏌?Gitee `/releases/latest`锛屾瘮瀵圭増鏈彿鏈夋柊鐗堝氨鎻愮ず + 銆屽幓涓嬭浇銆嶈烦娴忚鍣ㄣ€傛棤 in-app 鑷姩瀹夎銆?
  - **鏇存柊妫€娴?HTTP 鏀?Rust锛坮eqwest锛変笉鏀惧墠绔?fetch銆?* 鍥犱负 `tauri.conf.json` CSP `connect-src 'self' ipc: http://ipc.localhost` 閿佸緱寰堢揣锛屽墠绔?fetch Gitee 浼氳鎷︼紱Rust reqwest 涓嶅彈 CSP 绾︽潫銆佽閬?CORS銆佸鐢ㄥ凡鏈変緷璧栥€傗啋 **鏃犻渶鏀惧 CSP**銆?
  - **鏇存柊鍐呭鏉ユ簮 = 鍐呯疆 CHANGELOG.md + Vite `?raw`銆?* 鏋勫缓鏈熷唴鑱旀垚瀛楃涓叉墦鍖呰繘鍓嶇锛岀绾垮彲鐢ㄣ€佷笌宸茶鐗堟湰涓ユ牸瀵瑰簲锛涚敤宸叉湁 `react-markdown`+`remark-gfm` 娓叉煋锛屼笉鏂板渚濊禆銆傛洿鏂版娴嬬殑銆屾柊鐗堟湰銆嶈蛋杩滅▼銆侀鍚€屾洿鏂板唴瀹广€嶈蛋鏈湴 CHANGELOG鈥斺€斾袱鏉＄嫭绔嬭矾寰勶紝鍐呭婧愪笉鍚岋紙杩滅▼=宸插彂甯冩渶鏂帮紝鏈湴=宸茶鐗堟湰鐨勫彉鏇达級銆?
  - **鎵撳紑澶栭摼 = Rust `open_external_url` 鍛戒护銆?* 澶嶇敤宸叉敞鍐岀殑 `tauri_plugin_shell::ShellExt`锛屽己鍒舵牎楠?`http(s)://` 鍓嶇紑锛堥槻 file:/浠绘剰鍗忚锛夛紝涓嶆柊澧?`@tauri-apps/plugin-shell` npm 鍖呫€俙Shell::open` 宸?deprecated锛堟帹鑽?tauri-plugin-opener锛夛紝浣嗕负鍗曟鎵撳紑涓嶅紩绗簩涓彃浠讹紝`#[allow(deprecated)]` 鐣欐敞璇存槑銆?
  - **鑺傛祦锛氭瘡 24h 鎵嶈嚜鍔ㄦ娴嬩竴娆?*锛坄localStorage lastUpdateCheck`锛夛紝閬垮厤姣忔鍚姩鎵?Gitee API 瑙﹀彂闄愭祦锛涖€屾鏌ユ洿鏂般€嶆寜閽笉鍙楄妭娴併€?*姣忕増鏈彧寮逛竴娆℃洿鏂版彁绀?*锛坄localStorage lastNotifiedUpdateVersion`锛夛紝鍚岀増鏈悗缁惎鍔ㄥ彧鐣欎晶鏍忕孩鐐逛笉寮圭獥銆?
  - **棣栧惎鏇存柊鍐呭锛坵hatsnew锛夛細** `localStorage knownVersion` vs 杩愯鐗堟湰锛屼笉鍚岋紙鍒氬崌绾э級鈫?寮?changelog 涓€娆★紝鍏抽棴鏃跺洖鍐?knownVersion锛涢娆″畨瑁咃紙knownVersion 涓?null锛夐潤榛樺啓鍏ヤ笉鎵撴壈銆?
  - **鐗堟湰鍙蜂綅缃?= 渚ц竟鏍忓簳閮?*锛堝父椹诲彲瑙侊紝鐐瑰嚮寮€ About/鏇存柊鍐呭瀵硅瘽妗嗭紝鏈夋柊鐗堟湰鏃舵樉绾㈢偣锛夛紱璁剧疆闈㈡澘銆岀増鏈浠姐€嶅尯鐗堟湰鍙蜂繚鎸佷笉鍙樸€?
  - **瀛椾綋瀛楁绉诲埌 ConnectionDialog 鏈€搴曢儴**锛堛€屽垎缁勩€岶ieldGroup 涔嬪悗锛夛紝浠呬綅缃Щ鍔ㄣ€佹潯浠朵笌閫昏緫涓嶅彉銆?
- **鏀瑰姩锛堝悗绔級锛?*
  - `main.rs` 鈥?`UpdateInfo` 缁撴瀯 + `GITEE_LATEST_RELEASE` 甯搁噺 + `check_for_updates()` async 鍛戒护锛坮eqwest 10s 瀹㈡埛绔?+ 12s tokio timeout 鍏滃簳 + serde_json::Value 鎵嬪彇瀛楁瀹归敊 + `is_newer`/`parse_version`/`truncate_chars`/`unix_now_secs`/`update_info_error` helper锛?*姘镐笉鎶?Result**锛屽け璐ュ叏閮ㄩ檷绾?has_update:false+error锛? `open_external_url()` 鍛戒护锛圓ppHandle 娉ㄥ叆 + http(s) 鏍￠獙 + `#[allow(deprecated)]` ShellExt::open锛夛紱涓ゅ懡浠ゆ敞鍐岃繘 `generate_handler!`銆備笉鏂板 Cargo 渚濊禆銆?
- **鏀瑰姩锛堝墠绔級锛?*
  - `api.ts` 鈥?`UpdateInfo` 鎺ュ彛 + `checkForUpdates()` + `openExternalUrl(url)`銆?
  - `vite-env.d.ts` 鈥?`declare module "*.md?raw"` 璁?`?raw` 瀵煎叆杩?tsc銆?
  - `hooks/useUpdateCheck.ts`锛堟柊锛夆€?`useUpdateCheck(enabled)` 鈫?`{info,loading,checkNow}`锛?4h localStorage 鑺傛祦锛沬nFlight ref 闃?StrictMode 鍙岃Е鍙戯紱澶辫触闈欓粯銆?
  - `components/AboutDialog.tsx`锛堟柊锛夆€?鍗曠粍浠朵袱 mode锛坵hatsnew/about锛夛紱ReactMarkdown + remarkGfm 娓叉煋 `CHANGELOG.md?raw`锛坕nline components map锛屾棤闇€鍏ㄥ眬 CSS 绫伙級锛沘bout 妯″紡鍚€屾鏌ユ洿鏂般€嶆寜閽?+ has_update 鏃剁豢 banner銆屽幓涓嬭浇銆嶃€?
  - `components/Sidebar.tsx` 鈥?鏂?props `version/updateAvailable/onOpenAbout`锛沗listContainer` 鍚庢彃 footer锛堜粎灞曞紑鎬侊級`MyShell v{version}` + 绾㈢偣锛涚偣鍑诲紑 about銆?
  - `App.tsx` 鈥?`appVersion` state锛坴ault ready 鍚?`getAppVersion`锛? whatsnew effect锛坘nownVersion 姣斿锛? `useUpdateCheck` + 姣忕増鏈竴娆℃彁绀?effect锛坙astNotifiedUpdateVersion锛? `closeAbout`锛坵hatsnew 鍏抽棴鍥炲啓 knownVersion锛夛紱娓叉煋 `<AboutDialog>`锛汼idebar 娉ㄥ叆 version/updateAvailable/onOpenAbout銆?
  - `components/ConnectionDialog.tsx` 鈥?銆岀粓绔€岶ieldGroup锛堝瓧浣擄級浠庡熀鏈缃笅绉诲埌銆屽垎缁勩€嶄箣鍚庛€佽〃鍗曞鍣ㄦ渶搴曘€?
  - `CHANGELOG.md`锛堟柊锛屼粨搴撴牴锛夆€?鐗堟湰鍖栨洿鏂版棩蹇楋紙v1.4.5 鈫?鍘嗗彶鍥炴函锛夛紝澶存敞鐗堟湰椤讳笌 Cargo.toml 涓€鑷淬€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛? warning锛夈€傞渶 `cargo tauri dev` 鎵嬫祴锛氫晶鏍忓簳閮ㄧ増鏈彿 + 鐐瑰嚮寮€ about + 妫€鏌ユ洿鏂帮紙Gitee 鏈夋洿楂樼増鏈啋缁?banner 鍘讳笅杞斤紱鏈€鏂扳啋銆屽綋鍓嶅凡鏄渶鏂扮増鏈€嶏級锛涙敼 `localStorage myshell.knownVersion` 涓烘棫鐗堝埛鏂扳啋寮?whatsnew锛涙娴嬪埌鏂扮増鏈鍚脊 about銆佸悗缁惎鍔ㄤ粎绾㈢偣锛涙柇缃?鏃?release 闈欓粯闄嶇骇鏃犲脊绐楁棤绾㈢偣锛涙柊寤鸿繛鎺ュ璇濇瀛椾綋鍦ㄦ渶搴曪紙SSH/Local 鏄剧ず銆丗TP/SFTP 涓嶆樉绀猴級銆?
- **杈圭晫/閿欒澶勭悊瑕佺偣锛?*
  - 鏇存柊妫€娴嬶細缃戠粶澶辫触/瓒呮椂/闈?2xx/瑙ｆ瀽澶辫触/鏃?tag 鈫?鍏ㄩ儴闄嶇骇 `error` 瀛楁闈炵┖ + has_update:false锛岀粷涓?panic/鎶?Result锛沶otes 鎴柇 2000 瀛楅槻鐖嗗墠绔紱download_url 缂虹渷鍥為€€ release_url锛汣SP 涓嶆斁瀹斤紙HTTP 璧?Rust锛夈€?
  - 鎵撳紑澶栭摼锛氶潪 http(s) 鍓嶇紑鎷掔粷锛汼hellExt 缂哄け杩斿洖銆屾棤娉曟墦寮€閾炬帴銆嶃€?
  - whatsnew锛氶娆″畨瑁咃紙knownVersion null锛夐潤榛樹笉寮癸紱鏂綉/妫€娴嬪け璐ヤ笉褰卞搷 whatsnew锛坵hatsnew 璧版湰鍦?CHANGELOG 涓嶄緷璧栫綉缁滐級銆?
  - 姣忕増鏈彁绀猴細whatsnew 姝ｆ樉绀烘椂涓嶆姠灞忥紙about 璁╀綅 whatsnew锛夛紱鍚?latest_version 鍚庣画鍚姩浠呯孩鐐广€?
  - 鑺傛祦閿啓澶辫触锛坙ocalStorage 婊?绂佺敤锛変笉褰卞搷妫€鏌ワ紝浠呭彲鑳戒笅娆″惎鍔ㄦ棭鏌ヤ竴娆°€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 39锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 39 complete锛堜晶鏍忕増鏈彿 + 杞婚噺鏇存柊妫€娴?鎻愮ず + 棣栧惎鏇存柊鍐呭 + 瀛椾綋绉诲簳锛涘悗绔?check_for_updates/open_external_url + parse_ver/is_newer/truncate helper + 涓ゅ懡浠ゆ敞鍐岋紱鍓嶇 UpdateInfo api 鍖呰 + useUpdateCheck 鑺傛祦 hook + AboutDialog 鍙?mode + CHANGELOG.md ?raw + Sidebar footer + App 瑁呴厤 + ConnectionDialog 瀛楁绉讳綅锛泃sc/cargo check 鍙岀豢锛夛紝寰?dev 鎵嬫祴 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴锛氣憼渚ф爮搴曢儴 `MyShell v1.4.5` + 鐐瑰嚮寮€ About锛涒憽About 鐐广€屾鏌ユ洿鏂般€嶁啋Gitee 鏈夋洿楂樼増鏈啋缁?banner銆屽幓涓嬭浇銆嶆墦寮€娴忚鍣ㄤ笅杞介〉 / 鏈€鏂扳啋銆屽綋鍓嶅凡鏄渶鏂扮増鏈€? 鏂綉鈫掋€屼笂娆℃鏌ュけ璐ュ彲閲嶈瘯銆嶏紱鈶localStorage.setItem("myshell.knownVersion","1.0.0")` 鍒锋柊鈫掑脊 whatsnew锛坈hangelog锛夛紝鍏抽棴鍚庡洖鍐欍€佸啀鍒锋柊涓嶅脊锛涒懀瀛椾綋瀛楁鍦ㄦ柊寤鸿繛鎺ュ璇濇鏈€搴曪紙SSH/Local 鏄剧ず銆丗TP/SFTP 涓嶆樉绀猴級锛涒懁姣忕増鏈彁绀轰竴娆★紙妫€娴嬪埌鏂扮増鏈鍚脊 about锛屽悗缁粎绾㈢偣锛?|
| 鐩爣鏄粈涔堬紵 | 鐗堟湰鍙峰父椹诲彲瑙侊紱鑷姩妫€娴嬫柊鐗堟湰骞舵彁绀轰竴娆★紙闈?naggy锛? 鍙墜鍔ㄦ鏌ワ紱鐗堟湰鍗囩骇鍚庨娆″惎鍔ㄥ睍绀烘洿鏂板唴瀹癸紱瀛椾綋瀛楁涓嶆尅甯哥敤杈撳叆鍖?|
| 鎴戝鍒颁簡浠€涔堬紵 | 鏇存柊妫€娴?HTTP 蹇呴』鏀?Rust鈥斺€擟SP 閿佺揣 connect-src锛屽墠绔?fetch 澶栫珯琚嫤锛孯ust reqwest 涓嶅彈 CSP 绾︽潫涓旇閬?CORS锛堝叧閿纭喅绛栵紝閬垮厤鏀惧瀹夊叏 CSP锛夛紱鏇存柊妫€娴嬭姘镐笉鎶?Result 鍏ㄩ檷绾?error 瀛楁锛堝墠绔绾﹀共鍑€锛夛紱鏇存柊鍐呭銆屽唴缃?CHANGELOG.md ?raw銆嶄笌銆岃繙绋?release body銆嶆槸涓ゆ潯鐙珛璺緞锛屽墠鑰呭搴斿凡瑁呯増鏈€佺绾垮彲鐢紝鍚庤€呭搴斿凡鍙戝竷鏈€鏂帮紙璇箟涓嶅悓涓嶈兘娣凤級锛涢鍚?whatsnew 鐢?knownVersion 姣斿锛岄娆″畨瑁咃紙null锛夎闈欓粯涓嶆墦鎵帮紱鎻愮ず瑕侀槻 naggy鈥斺€?4h 鑺傛祦 + 姣忕増鏈彧寮逛竴娆★紙lastNotifiedUpdateVersion锛夛紱杞婚噺妫€娴?鎻愮ず浼樹簬闆嗘垚 tauri-plugin-updater锛堝厤绛惧悕瀵嗛挜/manifest/CI锛屽鍚?Gitee 鎵樼锛夛紱鎵撳紑澶栭摼澶嶇敤宸叉敞鍐岀殑 shell plugin 涓嶅紩鏂?npm 鍖咃紝浠?`#[allow(deprecated)]` 搴斿涓婃父杩佺Щ opener锛涘瓧浣撳瓧娈电Щ浣嶆槸绾綅缃敼鍔ㄩ浂閫昏緫椋庨櫓 |
| 鎴戝仛浜嗕粈涔堬紵 | main.rs UpdateInfo+GITEE_LATEST+check_for_updates锛坮eqwest+timeout+serde_json 鎵嬪彇+姘镐笉鎶涳級+open_external_url锛坔ttp(s) 鏍￠獙+ShellExt+allow deprecated锛?parse_ver/is_newer/truncate_chars/unix_now_secs/update_info_error helper+涓ゅ懡浠ゆ敞鍐岋紱api.ts UpdateInfo/checkForUpdates/openExternalUrl锛泇ite-env.d.ts `*.md?raw` 澹版槑锛泆seUpdateCheck.ts锛?4h 鑺傛祦+inFlight ref+闈欓粯锛夛紱AboutDialog.tsx锛坵hatsnew/about 鍙?mode+ReactMarkdown inline components锛夛紱Sidebar.tsx version footer+绾㈢偣+onOpenAbout锛汚pp.tsx appVersion+whatsnew effect+useUpdateCheck+姣忕増鏈彁绀?effect+closeAbout+娓叉煋锛汣onnectionDialog.tsx 瀛椾綋 FieldGroup 绉诲簳锛汣HANGELOG.md 鐗堟湰鍖栫瀛愶紱tsc+cargo check 鍙岀豢锛沺rogress+README 鍚屾 |

### 闃舵 40锛氬彂甯冩祦姘寸嚎 + 寮€婧愬畨鍏ㄥ璁★紙2026-06-24锛?
- **鍙戝竷娴佹按绾匡紙涓€閿?`鎵撳寘`锛夛細** 鏂板 `scripts/publish-gitee-release.mjs`锛圙itee API 鍒涘缓 release + `attach_files` 涓婁紶 .exe锛孨ode 18+ fetch锛屽甫 4 娆＄綉缁滈噸璇曪級锛沗.gitee-token` gitignored锛堟案涓嶈繘浠撳簱锛夛紱CLAUDE.md 鍔犮€屾墦鍖呫€嶈鍒?+ 鎸佷箙璁板繂銆傝鍒欐紨杩涳細鍒濈増鍏ㄨ嚜鍔?鈫?鐢ㄦ埛鍔?*纭闂?*锛堝啓瀹屾洿鏂板唴瀹瑰悗鍋滀笅锛岀粰浣滆€呯‘璁ょ増鏈彿+changelog 鎵嶇户缁?build/push/publish锛夈€傚涓诲畨鍏ㄥ垎绫诲櫒鐙珛鎷?push-to-main 涓庡叕寮€ release 鍙戝竷锛堜笌鐢ㄦ埛鎺堟潈鏃犲叧锛夛紝鐢?`!` 鍓嶇紑浜叉墜鏀捐銆?*韪╁潙**锛歚cargo tauri` 瀛愬懡浠ゆ湭瑁呪啋鏀?`npm run tauri:build`锛沗AboutDialog` 鐨?`../CHANGELOG.md?raw` 璺緞閿欙紙搴旀槸 `../../`锛宼sc 娌℃姄鍒般€乿ite 鎵撳寘鎵嶆毚闇诧級銆?
- **寮€婧愬畨鍏ㄥ璁★細** 鍏ㄥ巻鍙叉壂瀵嗛挜/鍙ｄ护/閭/IP/鍐呯綉涓绘満鍚嶃€傜粨璁衡€斺€擿.gitee-token` 浠庢湭杩涘巻鍙诧紙瀹夊叏锛夈€佹棤纭紪鐮佸瘑閽ャ€乣ai.rs` 鏃犻粯璁?key銆乣.serena/` 鍙窡韪粯璁ら厤缃€佹椿鏂囨。鏃犺繍钀ヤ俊鎭€?*淇**锛? 涓剼鏈噷纭紪鐮?`C:\Users\argus\.cargo\bin`鈫抈%USERPROFILE%`/`$HOME`锛堝彲绉绘+鑴辨晱锛夛紱`TerminalPanel.tsx` 娉ㄩ噴 `argus@fn-na`鈫抈user@host`銆?*鍐崇瓥**锛氫綔鑰呰韩浠斤紙`argus<argustang@qq.com>`/`Dear鍞愬厛鐢焋/`tang.li`锛夌敤鎴烽€夋嫨淇濈暀锛涙晠**璺宠繃鍘嗗彶閲嶅啓**锛堜綔鑰呴偖绠卞凡鍏ㄥ叕寮€锛屾竻鏃ц剼鏈噷鐨?argus 鏀剁泭鈮?锛屼笉鍊煎己鎺ㄩ闄╋級銆傛畫鐣欐毚闇查潰锛氫綔鑰?QQ 閭+濮撳悕鍦ㄦ彁浜ゅ厓鏁版嵁锛堝凡鐭ユ儏淇濈暀锛夈€侺ICENSE 瀹炰负 Apache 2.0 浣?README 鍐?MIT锛堟湭淇紝寮€婧愯鑼冨皬鐟曠柕锛夈€?
- **鍙戠増锛?* v1.5.0锛坒eature锛氱増鏈彿灞曠ず+鑷姩妫€娴嬫洿鏂?棣栧惎鏇存柊鍐呭+瀛椾綋绉诲簳锛宺elease 723458锛夆啋 v1.5.1锛坧atch锛氬紑婧愭暣鐞嗭紝浜岃繘鍒朵笌 1.5.0 绛変环锛宺elease 723544锛夈€傞娆?1.5.0 鍙戝竷鍚?2 娆?Gitee 缃戠粶瓒呮椂锛堜績鎴愯剼鏈姞 retry锛夈€?
- **楠岃瘉锛?* `npx tsc --noEmit` + `cargo check` 鍙岀豢锛涗袱涓?release 鍧囧凡鍙戝竷鍚?.exe銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 40锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 40 complete锛堝彂甯冩祦姘寸嚎 publish-gitee-release.mjs + 鎵撳寘瑙勫垯鍚‘璁ら棬 + 寮€婧愬璁′慨澶?+ v1.5.0/v1.5.1 鍙屽彂鐗堬級锛屽潎宸叉帹閫?origin/main |
| 鎴戣鍘诲摢閲岋紵 | 鍚庣画鍙戠増鐩存帴銆屾墦鍖呫€嶁啋纭鏇存柊鍐呭鈫掕嚜鍔?build+push+鍙?Gitee锛涘紑婧愬悗鍏虫敞鏄惁鏈夐仐婕?PII 鍙嶉 |
| 鐩爣鏄粈涔堬紵 | 涓€閿彲澶嶇幇鍙戠増锛涘紑婧愬墠娓呮帀涓汉璺緞/鑴辨晱锛涙棦淇濈暀浣滆€呰韩浠藉張璁╀粨搴撳澶栧共鍑€ |
| 鎴戝鍒颁簡浠€涔堬紵 | 鍙戝竷鑴氭湰浜岃繘鍒朵笂浼犵敤 Gitee `attach_files`锛堥潪 GitHub upload_url锛夛紱Node fetch 鍋?multipart 鏈€绋筹紙椤圭洰宸蹭繚璇佹湁 Node锛夛紱瀹夊叏鍒嗙被鍣ㄧ嫭绔嬩簬鐢ㄦ埛鎺堟潈浼氭嫤 push-main/鍏紑鍙戝竷锛岀敤 `!` 鏀捐锛沗?raw` 瀵煎叆璺緞瑕佺畻瀵瑰眰绾с€乼sc 涓嶆煡杩愯鏈熻矾寰勫彧鏌ョ被鍨嬶紱寮€婧愬璁¤鎵?*鍏ㄥ巻鍙?*鑰岄潪浠呭伐浣滃尯锛堜絾鍘嗗彶閲嶅啓鏀剁泭瑕佺湅浣滆€呰韩浠芥槸鍚︿繚鐣欌€斺€斾繚鐣欏垯娓呮枃浠跺唴瀹规敹鐩娾増0锛夛紱CRLF 璀﹀憡鏃犲锛圵indows锛夛紱纭闂ㄦ槸濂借璁♀€斺€旀棤鐢ㄦ埛鏀瑰姩鏃跺瀹炲缓璁烦杩囥€佺敤鎴蜂粛瑕佸彂灏卞彂 |
| 鎴戝仛浜嗕粈涔堬紵 | publish-gitee-release.mjs锛坈reate release+attach_files+4娆￠噸璇?token 瑙ｆ瀽 env/鏂囦欢锛夛紱.gitignore 鍔?.gitee-token锛汣LAUDE.md 鎵撳寘瑙勫垯锛?姝ュ惈 3.5 纭闂?鍒嗙被鍣ㄦ斁琛屾彁绀猴級+璁板繂/绱㈠紩鍚屾锛? 鑴氭湰 cargo 璺緞鑴辨晱+TerminalPanel 娉ㄩ噴鑴辨晱锛堟彁浜?8ac7606锛夛紱鍙戠増 v1.5.0(723458)/v1.5.1(723544) 鍚?.exe锛汣HANGELOG/README/progress 鍚屾 |

### 闃舵 41锛氭瘡娆＄櫥褰曟鏌ユ洿鏂?+ 宸︿笅瑙掓彁绀?+ 鑷姩涓嬭浇瀹夎锛?026-06-24锛?
- **闇€姹傦紙鐢ㄦ埛鍘熻瘽锛夛細** 姣忔鐧诲綍杩涘叆涓婚〉鍚庢鏌ヤ竴娆℃柊鐗堟湰锛涚綉缁滆缃?30s 瓒呮椂锛涙鏌ュ埌鏂扮増鏈啋宸︿笅瑙掓彁绀恒€屽彂鐜版柊鐗堟湰銆?缁跨偣锛涚敤鎴风‘璁ゅ悗鑷姩涓嬭浇骞跺畨瑁咃紝涓嬭浇澶辫触鍐嶇敱鐢ㄦ埛鐐瑰嚮椤甸潰涓嬭浇瀹夎銆?
- **鏂规閫夊瀷锛?* 鑷姩瀹夎涓ら€夆€斺€擜 鍚庣涓嬭浇 .exe + 鍚姩 NSIS 瀹夎鍣紙鏃犵鍚嶏紝HTTPS+鐢ㄦ埛纭锛夛紱B 闆嗘垚 tauri-plugin-updater锛堢鍚嶅瘑閽?manifest+CI 鏀归€狅級銆傞€?**A**锛堝鍚堢幇鏈?Gitee release 娴佹按绾裤€佸厤绛惧悕鍩哄缓锛夛紝涓嬭浇澶辫触鍥為€€娴忚鍣ㄤ笅杞斤紙鐢ㄦ埛鍘熻瘽锛夈€?
- **鏀瑰姩锛堝悗绔級锛?*
  - `main.rs` 鈥?`check_for_updates` 瓒呮椂 10s鈫?*30s**锛坈lient builder timeout 瑕嗙洊鍏ㄨ姹傜敓鍛藉懆鏈燂紝鍘绘帀澶氫綑 tokio wrapper锛夛紱鏂板 `download_update(window,url)`锛歳eqwest 娴佸紡涓嬭浇鍒?`temp_dir/myshell-update-setup.exe`锛宍bytes_stream` 閫愬潡鍐?tokio fs锛屾瘡 256KB `window.emit("update_download_progress",{downloaded,total})` 鑺傛祦锛屾湯灏捐ˉ鍙?100%锛岃繑鍥炰复鏃惰矾寰勶紱鏂板 `install_update(app,path)`锛氭牎楠岃矾寰勯潪 NUL/鏄枃浠?`.exe` 鍚庣紑锛學indows 鐢?`CommandExt::creation_flags(DETACHED_PROCESS|CREATE_NEW_PROCESS_GROUP)` 鍒嗙鍚姩锛屽啀 `app.exit(0)` 璁╁畨瑁呭櫒鎺ョ鏇挎崲鏂囦欢锛涗袱鍛戒护娉ㄥ唽 `generate_handler!`锛沗use tauri::Emitter`锛坋mit 鏂规硶鎵€鍦?trait锛?.11 鎷嗗嚭锛夈€?
- **鏀瑰姩锛堝墠绔級锛?*
  - `api.ts` 鈥?`DownloadProgress` 鎺ュ彛 + `downloadUpdate(url)`/`installUpdate(path)`/`onUpdateDownloadProgress(handler)`銆?
  - `hooks/useUpdateCheck.ts` 鈥?**鍘绘帀 24h 鑺傛祦**锛屾敼銆屾瘡浼氳瘽锛堟瘡娆?enabled鈫抰rue 鍗崇櫥褰曡繘涓婚〉锛夎嚜鍔ㄦ鏌ヤ竴娆°€峘autoRanRef`锛涗繚鐣?`checkNow` 鎵嬪姩銆?
  - `components/UpdateNotification.tsx`锛堟柊锛夆€?宸︿笅瑙?fixed 鍗＄墖锛坄left:16;bottom:16`锛夛紝缁跨偣 +銆屽彂鐜版柊鐗堟湰 vX.X.X銆?鉁?蹇界暐锛涘洓鎬佹満 prompt鈫抎ownloading(杩涘害鏉★紝璁㈤槄浜嬩欢)鈫抮eady(瀹夎骞堕噸鍚?鈫抐ailed(娴忚鍣ㄤ笅杞?閲嶈瘯)锛涘拷鐣ユ寜 `latest_version` 瀛?localStorage锛堟洿鏂扮増鏈啀鐜帮級銆?
  - `App.tsx` 鈥?鍒犳棫鐨勩€屾瘡鐗堟湰寮逛竴娆?About 妯℃€併€峞ffect锛堟崲鎴愭洿杞荤殑宸︿笅瑙掑崱鐗囷級锛涙覆鏌?`<UpdateNotification>`锛坴ault ready + has_update 鏃讹級锛涗繚鐣欎晶鏍忕豢鐐?+ About 鎵嬪姩妫€鏌?+ whatsnew 鍗囩骇鍚庢洿鏂版棩蹇椼€?
- **楠岃瘉锛?* `npx tsc --noEmit` + `cargo check` 鍙岀豢銆傞渶 dev 鎵嬫祴锛氱櫥褰曞悗鑷姩妫€鏌ワ紱Gitee 鏈夋洿楂樼増鏈啋宸︿笅瑙掑崱鐗?缁跨偣锛涚偣銆岀珛鍗虫洿鏂般€嶁啋杩涘害鏉♀啋銆屽畨瑁呭苟閲嶅惎銆嶁啋App 閫€鍑?NSIS 瀹夎鍣?UAC)鎺ョ锛涙柇缃?涓嬭浇澶辫触鈫掋€屾祻瑙堝櫒涓嬭浇銆嶅洖閫€銆?
- **杈圭晫/閿欒澶勭悊瑕佺偣锛?*
  - 涓嬭浇锛歶rl 蹇呴』 http(s)锛汬TTP 闈?2xx/璇绘祦澶辫触/鍐欑洏澶辫触鈫抈Err`锛屽墠绔繘 failed 鎬佸洖閫€娴忚鍣紱涓嬭浇 client timeout 300s锛堝畨瑁呭寘鍑?MB 浣嗘參缃戣缁欎綑閲忥級锛涜繘搴?total=0锛堟棤 Content-Length锛夋椂 UI 鏄剧ず銆屼笅杞戒腑鈥︺€嶆棤鐧惧垎姣斻€?
  - 瀹夎锛氳矾寰勫惈 NUL/闈炴枃浠?闈?`.exe` 鎷掔粷锛堥槻琚綋浠绘剰鏂囦欢鍚姩鍣級锛沘pp.exit(0) 鍚庡畨瑁呭櫒鐙珛杩愯锛沺erMachine 瀹夎鍣ㄨЕ鍙?UAC锛堟甯?Windows 瀹夎 UX锛夈€?
  - 鎻愮ず闃?naggy锛氬拷鐣ユ寜鐗堟湰璁板繂锛屽悓鐗堟湰蹇界暐鍚庝笉鍐嶅脊锛屾洿鏂扮増鏈墠鍐嶇幇锛涗晶鏍忕豢鐐瑰父椹?+ 鍗＄墖鍙叧銆?
  - 瀹夊叏锛欰 鏂规鏃犵鍚嶆牎楠岋紝闈?HTTPS锛圙itee TLS锛? 鐢ㄦ埛鏄惧紡纭锛涙湭鏉ラ槻 MITM 鍐嶄笂 B锛坱auri-plugin-updater 绛惧悕鏍￠獙锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 41锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 41 complete锛堟瘡娆＄櫥褰曟鏌?+ 30s 瓒呮椂 + 宸︿笅瑙掓彁绀哄崱鐗?+ 鑷姩涓嬭浇瀹夎鍣?澶辫触鍥為€€娴忚鍣紱鍚庣 check 瓒呮椂 30s + download_update 娴佸紡甯﹁繘搴?+ install_update 鍒嗙鍚姩+exit锛涘墠绔?api 鍖呰 + useUpdateCheck 鏀规瘡浼氳瘽 + UpdateNotification 鍥涙€佹満 + App 瑁呴厤鍘绘棫寮圭獥锛泃sc/cargo check 鍙岀豢锛夛紝寰?dev 鎵嬫祴鏁存潯鏇存柊閾捐矾 |
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛 `cargo tauri dev` 瀹炴祴锛氣憼鐧诲綍鍚庤嚜鍔ㄦ鏌ワ紙涓嶅啀 24h 鑺傛祦锛夛紱鈶itee 鍙戞洿楂樼増鏈啋宸︿笅瑙掑崱鐗囥€屽彂鐜版柊鐗堟湰 vX.X.X銆?缁跨偣锛涒憿鐐广€岀珛鍗虫洿鏂般€嶁啋杩涘害鏉″埌 100%鈫掋€屽畨瑁呭苟閲嶅惎銆嶁啋App 閫€鍑恒€丯SIS 瀹夎鍣?UAC)鎺ョ瀹夎锛涒懀鏂綉鎴栦笅杞藉け璐モ啋銆屾祻瑙堝櫒涓嬭浇銆嶆墦寮€ release 椤?+ 銆岄噸璇曘€嶏紱鈶ょ偣鉁曞拷鐣ヨ鐗堟湰鍚庝笉鍐嶅脊 |
| 鐩爣鏄粈涔堬紵 | 鐧诲綍鍗虫鏌ャ€?0s 瓒呮椂銆佸乏涓嬭闈炴墦鎵版彁绀恒€佺‘璁ゅ悗鑷姩涓嬭浇瀹夎銆佸け璐ュ洖閫€鎵嬪姩 |
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri 2.11 鎶?emit 鎷嗗埌 `Emitter` trait 闇€鍗曠嫭 import锛堟姤閿欐墠鐭ワ級锛況eqwest `bytes_stream` + tokio fs 娴佸紡涓嬭浇 + 姣?256KB 鑺傛祦 emit 杩涘害鏄共鍑€妯″紡锛沇indows 鍚姩澶栭儴瀹夎鍣ㄨ `DETACHED_PROCESS|CREATE_NEW_PROCESS_GROUP` 鎵嶅湪鐖惰繘绋?exit 鍚庡瓨娲伙紱鑷姩瀹夎閫夈€屼笅杞?鍚姩瀹夎鍣ㄣ€?A) 鑰岄潪 plugin-updater(B) 鏄负濂戝悎鐜版湁 Gitee release 鍏嶇鍚嶏紱鐢ㄣ€屾瘡浼氳瘽 autoRanRef銆嶅疄鐜般€屾瘡娆＄櫥褰曟鏌ヤ竴娆°€嶆瘮 localStorage 鑺傛祦鏇磋创鍚堣涔夛紱UI 鐘舵€佹満 prompt/downloading/ready/failed 璁╀笅杞?瀹夎-澶辫触鍥為€€涓夌璺緞娓呮櫚 |
| 鎴戝仛浜嗕粈涔堬紵 | main.rs check 瓒呮椂 30s+鍘?tokio wrapper銆乨ownload_update锛坮eqwest stream+tokio fs+256KB 鑺傛祦 emit+鏈熬 100%锛塱nstall_update锛圢UL/鏂囦欢/.exe 鏍￠獙+creation_flags 鍒嗙+app.exit锛夈€丒mitter import銆佷袱鍛戒护娉ㄥ唽锛沘pi.ts DownloadProgress/downloadUpdate/installUpdate/onUpdateDownloadProgress锛泆seUpdateCheck 鍘?24h 鑺傛祦鏀?autoRanRef 姣忎細璇濅竴娆★紱UpdateNotification.tsx 宸︿笅瑙掑崱鐗囧洓鎬佹満+杩涘害鏉?鎸夌増鏈拷鐣ワ紱App.tsx 鍒犳瘡鐗堟湰寮?About effect+娓叉煋 UpdateNotification锛泃sc+cargo check 鍙岀豢锛沺rogress+README 鍚屾 |

### 闃舵 42锛氭洿鏂版棩蹇楁敞閲婃硠闇蹭慨澶?+ 鍙戝竷鏆傚瓨缂撳啿鍖烘柟妗堬紙2026-06-24锛?
- **闇€姹傦細** 鈶犮€屾洿鏂板唴瀹广€嶅璇濇鎶?CHANGELOG.md 澶撮儴 HTML 娉ㄩ噴 `<!-- 鐗堟湰鍙烽』涓庘€?-->` 褰撴枃鏈樉绀轰簡锛岃鍘绘帀锛涒憽璁ㄨ骞惰惤鍦扮渷 token 鐨勬洿鏂板唴瀹圭敓鎴愭柟妗堚€斺€旀瘡瀹屾垚涓€椤规敼鍔ㄨ拷鍔犲埌鏆傚瓨 md锛屾墦鍖呮椂鍩轰簬鏆傚瓨鎬荤粨銆佷笉鍐嶅弽鎺?git diff锛屾墦鍖呭悗娓呯┖銆?
- **bug 淇锛?* `AboutDialog.tsx` 娓叉煋 CHANGELOG 鍓嶇敤 `cleanChangelog = changelog.replace(/<!--[\s\S]*?-->/g,"").replace(/\n{3,}/g,"\n\n").trim()` 鍓ユ帀鎵€鏈?HTML 娉ㄩ噴锛堜笉渚濊禆 react-markdown 瀵规敞閲婄殑澶勭悊琛屼负锛屾渶绋筹級銆?
- **鍙戝竷鏆傚瓨鏂规锛堢敤鎴风‘璁や笁鐐癸細绾冲叆 git / 缁?doc-after-feature / git diff 鍏滃簳锛夛細**
  - 鏂板 `RELEASE_NOTES_STAGING.md`锛堜粨搴撴牴锛実it 璺熻釜锛夆€?terse 缂撳啿鍖猴細`baseline: v1.6.0` + 銆屽緟鍙戝竷鏉＄洰銆嶏紙涓€琛屼竴鏉?`- <emoji> <涓€鍙ヨ瘽>`锛夈€?
  - CLAUDE.md `doc-after-feature` 瑙勫垯鎵╁睍锛氭瘡瀹屾垚鏀瑰姩锛屽啓 progress.md 闃舵鏃ュ織鐨?*鍚屾椂**杩藉姞涓€琛屽埌 staging銆?
  - CLAUDE.md `鎵撳寘` 娴佺▼鏀瑰啓锛氱増鏈彿鐢?staging 鏉＄洰绫诲瀷瀹氾紙鏈?鉁ㄢ啋minor锛岀函淇鈫抪atch锛夛紱鏇存柊鍐呭浠?staging 涓?*涓?*銆乣git diff --stat baseline..HEAD` 浠呬綔瀹屾暣鎬ф牎楠岄槻婕忥紱鍙戝竷鍚庢竻绌?staging 寰呭彂甯冩潯鐩苟鏇存柊 baseline銆?
  - 璁板繂 `release-notes-staging.md` + MEMORY.md 绱㈠紩鍚屾銆?
- **鍐崇瓥瑕佺偣锛?* 銆屼繚璇佹瘡娆￠棶绛旈兘璁板綍銆嶆棤娉曟満姊板己鍒讹紙闇€ LLM 鍒ゆ柇璁颁笉璁?鎬庝箞姒傛嫭锛夛紝闈犺涓鸿鍒欌€斺€旂粦宸叉湁 doc-after-feature 瑙﹀彂鐐规惌杞︼紝鍙潬鎬ф渶楂橈紱staging 鍙浼氳瘽鍐呮敼鍔紝鏁呬繚鐣?git diff 鍏滃簳闃叉紡銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆俿taging 宸茬鍏ユ湰娆?bug 淇鏉＄洰锛屽緟涓嬫鎵撳寘锛坴1.6.1 patch锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 42锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 42 complete锛圕HANGELOG 娉ㄩ噴娉勯湶淇 + 鍙戝竷鏆傚瓨鏂规钀藉湴锛歊ELEASE_NOTES_STAGING.md + CLAUDE.md 瑙勫垯 + 璁板繂锛泃sc 缁匡級锛宻taging 宸茬 bug 淇鏉＄洰锛屽緟鎵撳寘 |
| 鎴戣鍘诲摢閲岋紵 | 涓嬫 `鎵撳寘` 鈫?璇?staging 鐩存帴鐢熸垚 v1.6.1 changelog锛堜笉鍐嶅弽鎺?diff锛? git diff 闃叉紡 + 鍙戝竷鍚庢竻绌?staging |
| 鐩爣鏄粈涔堬紵 | 鏇存柊鍐呭瀵硅瘽妗嗕笉娉勯湶 HTML 娉ㄩ噴锛涚渷 token銆佹洿鍑嗐€佸彲澶嶇幇鐨?changelog 鐢熸垚娴佺▼ |
| 鎴戝鍒颁簡浠€涔堬紵 | react-markdown 瀵?HTML 娉ㄩ噴鐨勫鐞嗕笉鍙潬锛屾覆鏌撳墠姝ｅ垯鍓?`<!-- -->` 鏈€绋筹紱銆屾瘡娆￠棶绛旇褰曘€嶆槸琛屼负瑙勫垯闈炴満姊板己鍒讹紝缁戝凡鏈夌ǔ瀹氳鍒欙紙doc-after-feature锛夋惌杞︽渶鍙潬锛泂taging 涓轰富 + git diff 鍏滃簳鏄?鍑嗙‘鎻忚堪"涓?涓嶆紡"鐨勪簰琛ョ粍鍚堬紱鐢ㄦ埛瑕佺殑鏄?token 鏁堢巼+鍑嗙‘鎬э紝浜嬪悗鍙嶆帹 diff 鏃㈣吹鍙堟槗澶辩湡 |
| 鎴戝仛浜嗕粈涔堬紵 | AboutDialog cleanChangelog 鍓ユ敞閲婏紱RELEASE_NOTES_STAGING.md锛坆aseline v1.6.0 + bug 淇绉嶅瓙鏉＄洰锛夛紱CLAUDE.md doc-after-feature 鍔犺拷鍔?staging + 鎵撳寘娴佺▼鏀瑰啓锛坰taging 涓轰富/diff 闃叉紡/鐗堟湰鍙锋寜绫诲瀷/鍙戝竷鍚庢竻绌?鏇存柊 baseline锛夛紱璁板繂 release-notes-staging + MEMORY 绱㈠紩锛沺rogress 闃舵42锛泃sc 缁?|

### 闃舵 43锛氬彂甯?v1.6.2 鈥斺€?鏇存柊娴佺▼鎵撶（ + 瀹夎绋冲畾鎬э紙2026-06-24锛?
- **闇€姹傦細** `鎵撳寘` v1.6.2銆傛湰娆″唴瀹瑰叏閮ㄤ负鏇存柊鍔熻兘鏈韩鐨勪綋楠屾墦纾ㄤ笌绋冲畾鎬т慨澶嶏紙鏉ヨ嚜 staging 5 鏉★級銆?
- **鍙戝竷鍐呭锛坰taging 涓诲 + git diff 闃叉紡锛? 鏉″叏鏄犲皠锛屾棤閬楁紡锛夛細**
  - 馃洜锔?鏇存柊寮圭獥绠€鍖栦负鍗曞眰鍗＄墖锛堝幓鍙屽眰鐜荤拑澶栧３锛夛紝鎸夐挳銆屽拷鐣ャ€?銆屾洿鏂般€嶏紱AboutDialog銆屽幓涓嬭浇銆嶁啋銆屾洿鏂般€嶏紙鑷姩涓嬭浇瀹夎锛屽け璐ュ洖閫€娴忚鍣級銆?
  - 馃悰 銆屽畨瑁呭苟閲嶅惎銆峯s error 740锛堣姹傜殑鎿嶄綔闇€瑕佹彁鍗囷級鈥斺€?闈炵鐞嗗憳鐢ㄦ埛鐐规洿鏂版姤閿欙紱鏀圭敤 `ShellExecuteW` + `runas` 瑙﹀彂 UAC 鎻愭潈锛坢ain.rs锛夈€?
  - 馃悰 鏇存柊寮圭獥鐗堟湰鍙锋樉绀?`vv1.6.1`锛坱ag 宸插惈 v 鍓嶇紑鏃堕噸澶嶆坊鍔狅級銆?
  - 馃悰 杩涘害鏉＄偣鍑绘洿鏂版椂鍏堣烦涓棿鍐嶄粠 0 寮€濮嬶紙`pct` 涓?null 鏃跺垵濮嬪搴?30% 鈫?0%锛夈€?
- **鐗堟湰鍙凤細** staging 鍏ㄦ槸 馃悰/馃洜锔?鏃?鉁?鈫?patch锛歷1.6.1 鈫?**v1.6.2**銆?
- **娴佺▼鎵ц锛?* 鐢ㄦ埛纭闂搁棬閫氳繃鍚?鈥斺€?`tsc` PASS / `cargo check` PASS / `tauri:build` 鎴愬姛锛坕nstaller `MyShell_1.6.2_x64-setup.exe` 5.99MB锛? commit `release: v1.6.2` + push / Gitee release id=724075 鍒涘缓 + 璧勪骇涓婁紶鎴愬姛 / staging 娓呯┖ + baseline鈫抳1.6.2銆?
- **鍐崇瓥瑕佺偣锛?* staging 娴佺▼棣栨瀹屾暣璺戦€氾紙v1.6.1 鏄惤鍦版柟妗堟椂绉嶇殑绉嶅瓙锛夛紱`git diff --stat 37a3dba..HEAD` 鏄剧ず 5 鏂囦欢鏀瑰姩锛岄€愪竴鏍稿鍧囪 staging 瑕嗙洊锛堝惈 App.tsx 鍒?1 琛?onOpenAbout 灞炲脊绐楃畝鍖栥€乵ain.rs 灞?740 淇锛夛紝鏃犻渶琛ユ潯鐩€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛沗tauri:build` exit 0锛涘彂甯?URL锛歨ttps://gitee.com/argustang/myshell/releases/tag/v1.6.2

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 43锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 43 complete 鈥斺€?v1.6.2 宸插彂甯冨埌 Gitee锛坕nstaller 涓婁紶鎴愬姛锛夛紝staging 宸叉竻绌恒€乥aseline鈫抳1.6.2锛宺elease: v1.6.2 + 娓呯┖ staging 涓や釜 commit 鍧囧凡 push 鍒?main |
| 鎴戣鍘诲摢閲岋紵 | 涓嬩竴椤规敼鍔ㄥ畬鎴愭椂鎸?doc-after-feature 杩藉姞 staging 鏉＄洰锛涗笅娆?`鎵撳寘` 鍐嶈蛋鍚屾祦绋嬶紙璇?staging鈫掑畾鐗堟湰鍙封啋changelog鈫掔‘璁ら椄闂ㄢ啋鏋勫缓鍙戝竷锛?|
| 鐩爣鏄粈涔堬紵 | 璁╃敤鎴锋洿鏂颁綋楠岄『婊戯紙寮圭獥涓嶈姳鍝ㄣ€佺増鏈彿姝ｇ‘銆佽繘搴︽潯涓嶈烦銆侀潪绠＄悊鍛樹篃鑳戒竴閿崌绾э級 |
| 鎴戝鍒颁簡浠€涔堬紵 | staging 椹卞姩鐨勫彂甯冩祦绋嬮娆＄鍒扮璺戦€氾紝5 鏉＄瀛愬叏閮ㄥ噯纭槧灏勩€佹棤琛ユ潯闇€姹傦紱涓存椂鍙戝竷璇存槑 `.release-notes-v1.6.2.md` 鐢ㄥ畬鍗冲純锛?gitignore 涓嶅湪锛屼絾鏈湴鏂囦欢鏈彁浜ゃ€佸悗缁彲鍒狅級锛沗git diff --stat baseline..HEAD` 浣滀负鍏滃簳纭疄鍙捣闃叉紡鏍￠獙銆佽繖娆￠浂琛ユ潯 |
| 鎴戝仛浜嗕粈涔堬紵 | 鐗堟湰 bump 1.6.2 + sync锛汣HANGELOG/README 鍔?v1.6.2 鑺?+ badge锛泃sc+cargo check+tauri:build锛沜ommit+push release: v1.6.2锛汫itee 鍙戝竷锛坕d=724075锛宨nstaller 涓婁紶锛夛紱staging 娓呯┖ + baseline鈫抳1.6.2 commit+push锛沺rogress 闃舵43 |

### 闃舵 44锛氬彂甯?v1.6.3 鈥斺€?琛ㄥ崟蹇呭～鏍￠獙 + AI 浜や簰缁嗚妭锛?026-06-25锛?
- **闇€姹傦細** `鎵撳寘` v1.6.3銆傛湰娆″唴瀹逛负琛ㄥ崟蹇呭～鏍￠獙浣撶郴涓?AI 闈㈡澘浜や簰缁嗚妭浼樺寲锛堟潵鑷?staging 6 鏉★級銆?
- **鍙戝竷鍐呭锛坰taging 涓诲 + git diff 闃叉紡锛? 鏉″叏鏄犲皠锛屾棤閬楁紡锛夛細**
  - 馃洜锔?AI 闈㈡澘缁堢杈撳嚭鎴柇锛氳嚜鍔ㄩ檮甯︾殑缁堢杈撳嚭闄愬埗 5000 瀛楃锛岃秴闀挎椂鏄剧ず鎴柇鎻愮ず锛圓iPanel.tsx `capRecentOutput`锛夈€?
  - 馃洜锔?AI 闈㈡澘寮曠敤鍐呭鎶樺彔锛氱敤鎴峰紩鐢ㄥ唴瀹归粯璁ゅ崟琛岋紝鍙屽嚮灞曞紑瀹屾暣鍐呭锛圡essageBubble 鍙屽嚮鍒囨崲锛夈€?
  - 馃洜锔?鏂板缓杩炴帴蹇呭～鏍￠獙锛氫繚瀛樻椂鏍囩孩蹇呭～瀛楁銆佽嚜鍔ㄨ仛鐒﹂涓湭濉」骞舵姈鍔紙ConnectionDialog validate() + fieldErrors + shakeNonce锛夈€?
  - 馃洜锔?杩炴帴鍚嶇О鑷姩濉厖浼樺寲锛氭墜鍔ㄤ慨鏀瑰悕绉板悗涓嶅啀琚富鏈鸿緭鍏ヨ鐩栵紙handleHostChange 鍩轰簬 prevHost 鍒ゆ柇锛夈€?
  - 馃洜锔?蹇嵎鍛戒护蹇呭～鏍囨敞锛氬悕绉板拰鍛戒护瀛楁鏍囩孩鏄熷彿锛屾湭濉啓鏃剁鐢ㄤ繚瀛橈紙QuickCommandsPanel field-error锛夈€?
  - 馃洜锔?鑷畾涔変富棰樿壊鏍￠獙锛欻EX 鏍煎紡鏍￠獙锛岄潪娉曡緭鍏ユ椂绂佺敤淇濆瓨锛圫ettingsPanel isValidHex + canSave锛夈€?
- **鐗堟湰鍙凤細** staging 鍏ㄦ槸 馃洜锔?鏃?鉁?鈫?patch锛歷1.6.2 鈫?**v1.6.3**銆?
- **娴佺▼鎵ц锛?* 鐢ㄦ埛纭闂搁棬閫氳繃鍚?鈥斺€?`tsc` PASS / `cargo check` PASS / `tauri:build` 鎴愬姛锛坕nstaller `MyShell_1.6.3_x64-setup.exe`锛? commit + push / Gitee 鍙戝竷 / staging 娓呯┖ + baseline鈫抳1.6.3銆?
- **鍐崇瓥瑕佺偣锛?* 5 涓枃浠舵敼鍔紙AiPanel/ConnectionDialog/QuickCommandsPanel/SettingsPanel/global.css锛夛紝閫愪竴鏍稿鍧囪 staging 瑕嗙洊锛屾棤闇€琛ユ潯鐩€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛沗tauri:build` exit 0锛涘彂甯?URL锛歨ttps://gitee.com/argustang/myshell/releases/tag/v1.6.3

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 44锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 44 complete 鈥斺€?v1.6.3 宸插彂甯冿紝staging 宸叉竻绌恒€乥aseline鈫抳1.6.3 |
| 鎴戣鍘诲摢閲岋紵 | 涓嬩竴椤规敼鍔ㄥ畬鎴愭椂鎸?doc-after-feature 杩藉姞 staging 鏉＄洰 |
| 鐩爣鏄粈涔堬紵 | 琛ㄥ崟浜や簰浣撻獙涓€鑷村寲锛氬繀濉瓧娈垫湁瑙嗚鍙嶉銆丄I 涓婁笅鏂囦笉婧㈠嚭 |
| 鎴戝鍒颁簡浠€涔堬紵 | ConnectionDialog 鐨?validate() 鏀逛负鍏ㄩ噺鏍￠獙锛堜笉 short-circuit锛夛紝閰嶅悎 fieldErrors + shakeNonce 瀹炵幇澶氬瓧娈靛悓鏃舵爣绾?+ 鎶栧姩锛涜繛鎺ュ悕绉拌嚜鍔ㄥ～鍏呯殑銆岄獞鎵嬫娴嬨€嶏紙name === prevHost锛夋瘮鍘熸潵鐨?touchedRef 鏇寸簿鍑?|
| 鎴戝仛浜嗕粈涔堬紵 | 鐗堟湰 bump 1.6.3 + sync锛汣HANGELOG/README 鍔?v1.6.3 鑺?+ badge锛泃sc+cargo check+tauri:build锛沜ommit+push锛汫itee 鍙戝竷锛泂taging 娓呯┖ + baseline鈫抳1.6.3锛沺rogress 闃舵44 |

### 闃舵 45锛氬彂甯?v1.7.0 鈥斺€?渚ф爮浜や簰澧炲己 + 浼氳瘽绠＄悊闈㈡澘 + 骞挎挱绾ц仈閲嶈繛锛?026-06-25锛?
- **闇€姹傦細** `鎵撳寘` v1.7.0銆傛湰娆″唴瀹逛负渚ф爮鎷栨嫿璋冨銆佷細璇?骞挎挱涓嬫媺闈㈡澘銆佸箍鎾骇鑱旈噸杩炪€丄I 閫夊尯鎶樺彔绛夋柊鍔熻兘锛屼互鍙婂箍鎾幓閲嶇‘璁ゃ€佽繛鎺ユ牎楠屽寮恒€佸椤?bug 淇銆?
- **鍙戝竷鍐呭锛坰taging 涓诲 + git diff 闃叉紡锛?1 鏉″叏鏄犲皠锛屾棤閬楁紡锛夛細**
  - 鉁?渚ф爮鎷栨嫿璋冨锛氳繛鎺ョ鐞嗛潰鏉挎敮鎸佹嫋鎷借皟鏁村搴︼紙瀹藉害鎸佷箙鍖栵級锛岄紶鏍囨偓鍋滆繛鎺ュ悕鍙煡鐪嬪叏绉板強涓绘満鍦板潃锛圫idebar drag handle + localStorage + Tooltip锛夈€?
  - 鉁?浼氳瘽绠＄悊闈㈡澘锛氭柊澧炪€屽綋鍓嶄細璇濄€嶄笌銆屽箍鎾€嶄笅鎷夐潰鏉匡紝鍙煡鐪嬪叏閮ㄦ爣绛鹃〉鍏ㄧО銆佸揩閫熷垏鎹笌鍏抽棴锛屾敮鎸佷竴閿竻鐞嗘墍鏈夋帀绾夸細璇濓紙SessionDropdownPanel + TabBar锛夈€?
  - 鉁?骞挎挱绾ц仈閲嶈繛锛氱偣鍑婚噸杩炰竴涓帀绾夸細璇濓紝鍚屽箍鎾粍鍏朵粬鎺夌嚎浼氳瘽鑷姩涓€璧烽噸杩烇紙App.tsx cascadeReconnect锛夈€?
  - 鉁?AI 閫夊尯棰勮鎶樺彔锛欰I 鍔╂墜閫夊尯棰勮榛樿鍗曡灞曠ず锛屽弻鍑诲睍寮€瀹屾暣鍐呭锛圓iPanel MessageBubble锛夈€?
  - 馃洜锔?骞挎挱閲嶅鍔犲叆纭锛氬悓杩炴帴閲嶅鍔犲叆骞挎挱缁勬椂寮瑰嚭纭瀵硅瘽妗嗭紝鏀寔銆屾湰娆′細璇濅笉鍐嶆彁閱掋€嶏紙BroadcastDupDialog + sessionStorage锛夈€?
  - 馃洜锔?杩炴帴瀵硅瘽妗嗘牎楠屽寮猴細蹇呭～椤规爣娉ㄧ孩鑹叉槦鍙凤紝鏍￠獙涓嶉€氳繃鏃跺瓧娈电孩妗嗘姈鍔ㄥ苟绮剧‘鎻愮ず缂哄け椤癸紙ConnectionDialog锛夈€?
  - 馃洜锔?涓婚鑹蹭笌蹇嵎鍛戒护鏍￠獙锛氳嚜瀹氫箟涓婚棰滆壊杈撳叆澧炲姞 hex 鏍煎紡鏍￠獙锛屽揩鎹峰懡浠ゅ繀濉」鏍囨敞鏄熷彿銆?
  - 馃悰 骞挎挱缁勯噸杩炶劚缁勶細淇骞挎挱缁勬垚鍛橀噸杩炲悗鍥?sessionId 鍙樻洿鑰岃劚缁勭殑闂銆?
  - 馃悰 杩炴帴鍚嶇О鍚屾涓柇锛氫慨澶嶈繛鎺ュ悕绉拌窡闅忎富鏈哄湴鍧€杈撳叆鏃堕涓瓧绗﹀悗鍋滄鍚屾鐨勯棶棰樸€?
  - 馃悰 AI 涓婁笅鏂囨孩鍑猴細AI 鍔╂墜鑷姩闄勫甫鐨勬渶杩戣緭鍑哄唴瀹归檺鍒朵负 5K 瀛楃锛岄€夊尯鍐呭涓嶅彈闄愩€?
- **鐗堟湰鍙凤細** staging 鍚?鉁ㄦ柊澧?鈫?minor锛歷1.6.3 鈫?**v1.7.0**銆?
- **娴佺▼鎵ц锛?* 鐢ㄦ埛纭闂搁棬閫氳繃鍚?鈥斺€?`tsc` PASS / `cargo check` PASS / `tauri:build` 鎴愬姛锛坕nstaller `MyShell_1.7.0_x64-setup.exe`锛? commit + push / Gitee 鍙戝竷 / staging 娓呯┖ + baseline鈫抳1.7.0銆?
- **鍐崇瓥瑕佺偣锛?* 鐢ㄦ埛鍙 CHANGELOG 鏀?鉁ㄦ柊澧?4 鏉★紝馃洜锔?馃悰 鏉＄洰璁板綍鍦?progress 浣嗕笉鍐欏叆 CHANGELOG銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛沗tauri:build` exit 0

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 45锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 45 complete 鈥斺€?v1.7.0 宸插彂甯冿紝staging 宸叉竻绌恒€乥aseline鈫抳1.7.0 |
| 鎴戣鍘诲摢閲岋紵 | 涓嬩竴椤规敼鍔ㄥ畬鎴愭椂鎸?doc-after-feature 杩藉姞 staging 鏉＄洰 |
| 鐩爣鏄粈涔堬紵 | 渚ф爮鍙皟瀹姐€佷細璇濋潰鏉跨粺涓€绠＄悊銆佸箍鎾骇鑱旈噸杩炲噺灏戞墜鍔ㄦ搷浣?|
| 鎴戝鍒颁簡浠€涔堬紵 | BroadcastDupDialog 鐢?sessionStorage 璁板繂"鏈涓嶅啀鎻愰啋"鍋忓ソ锛堥噸鍚噸缃級锛汼essionDropdownPanel 缁熶竴绠＄悊褰撳墠浼氳瘽鍜屽箍鎾粍锛涘箍鎾骇鑱旈噸杩為€氳繃 groupId 杩囨护鍚岀粍鎺夌嚎浼氳瘽 |
| 鎴戝仛浜嗕粈涔堬紵 | 鐗堟湰 bump 1.7.0 + sync锛汣HANGELOG/README 鍔?v1.7.0 鑺?+ badge锛泃sc+cargo check+tauri:build锛沜ommit+push锛汫itee 鍙戝竷锛泂taging 娓呯┖ + baseline鈫抳1.7.0锛沺rogress 闃舵45 |


### 闂冭埖顔?46閿涙艾褰傜敮?v1.9.0 閳ユ柡鈧?缂佸牏顏〒鍙夌厠閸嬨儱锛庨幀?+ 閸忋劑鎽肩捄顖涙）韫囨娲冮幒褝绱?026-06-29閿?- **闂団偓濮瑰偊绱?* 閹恒儴绻涙稉銈勯嚋閺€鐟板З閺€璺虹啲閸欐垵绔烽妴鍌楁喖娣囶喖顦查柈銊ュ瀻閻劍鍩涚紒鍫㈩伂閸忓鐖?闁灏稉宥呭讲鐟欎緤绱檟term 濞撳弶鐓嬮崳銊ョ湴闂傤噣顣介敍澶涚幢閳垛€虫倵閸欑増妫╄箛妤冩磧閹貉冾杻瀵尨绱濇笟澶哥艾瀵倸鐖剁拠濠冩焽閵嗗倿妾敮锔藉Ω鐠佸墽鐤嗛棃銏℃緲闁插秵鐎稉鍝勫瀻缁顕遍懜顏傗偓?- **閸欐垵绔烽崘鍛啇閿涘澃taging 娑撶粯绨敍澶涚窗**
  - 閴併劍鏌婃晶?鐠佸墽鐤嗛棃銏℃緲瀹革箑褰搁崚鍡樼埉鐎佃壈鍩呴敍鍫濐樆鐟?AI/鐎瑰鍙?閺佺増宓佺粻锛勬倞/韫囶偅宓庨崨鎴掓姢閿?  - 閴併劍鏌婃晶?缂佸牏顏〒鍙夌厠閸氬海顏崣顖炩偓澶涚礄閼奉亜濮?Canvas/WebGL/DOM閿?  - 閴併劍鏌婃晶?閸忋劌鐪弮銉ョ箶閻╂垶甯堕敍鍫濆缁旑垰绌垮┃?閺堫亝宕熼懢宄扮磽鐢瓕娴嗛崣鎴濆煂閸氬骸褰撮弮銉ョ箶閿?  - 棣冩礈閿斿繋绱崠?缂佸牏顏妯款吇閺€鍦暏 Canvas 濞撳弶鐓嬮崳?+ 閺夆€宠埌閸忓鐖ｉ敍鍧坲rsorStyle:bar閿涘绱濋崗澶嬬垼缁嬪啿鐣鹃崣顖濐潌娑撴柧绻氶悾娆撴／閻?  - 棣冩礈閿斿繋绱崠?閸氬海顏崗鎶芥暛鐠侯垰绶炵悰銉ュ弿閺冦儱绻旈敍鍦玈H/SFTP/閺堫剙婀寸紒鍫㈩伂/閺佺増宓佹惔?婢跺洣鍞?AI/娴狅絿鎮婇敍?  - 棣冩礈閿斿繋绱崠?鐠佸墽鐤嗛弬鏉款杻閵嗗瞼顩﹂悽?GPU 绾兛娆㈤崝鐘烩偓鐔粹偓宥呯磻閸忕绱欓柌宥呮儙閻㈢喐鏅ラ敍瀛廝U 閸氬牊鍨氬鍌氱埗閸忔粌绨抽敍?  - 棣冩偘娣囶喖顦?forceVisibleCursor 濞夈劑鍣存竟鐗堟 cursorStyle:bar 娴ｅ棔鍞惍浣风矤閺堫亣顔曠純顔炬畱闁鏆€缂傛椽娅?
- **閻楀牊婀伴崣鍑ょ窗** staging 閸?閴併劍鏌婃晶?閳?minor閿涙1.8.0 閳?**v1.9.0**
- **濞翠胶鈻奸幍褑顢戦敍?* 鐎瑰本鏆ｉ幀褎鐗抽弻銉礄18 閺傚洣娆㈤崥?2 閺傜増鏋冩禒璁圭礉閸忋劌婀銉ょ稊閸栫儤婀幓鎰唉閿涘苯宕熼悪?git status 濡偓閺屻儻绱氶埆?閻劍鍩涚涵顔款吇闂傘劑鈧俺绻?閳?tsc PASS / cargo check PASS / tauri:build exit 0閿涘湣yShell_1.9.0_x64-setup.exe閿涘鍟?commit+push閿涘澁elease: v1.9.0, 21 閺傚洣娆?+1064/-133閿涘鍟?Gitee 閸欐垵绔?release id=728056 閳?staging 濞撳懐鈹?+ baseline閳姵1.9.0
- **閸愬磭鐡ョ憰浣哄仯閿?* 閳剁姴鍘滈弽鍥６妫版ɑ鐗撮崶鐘叉躬 xterm.js 濞撳弶鐓嬮崳銊ョ湴閿涘湹ebGL 閸忓鐖ｉ崣鐘插鐏炲倸鎮庨幋鎰亼鐠愩儯鈧笍OM 濞撳弶鐓嬮崳?focus 娓氭繆绂嗛敍澶涚礉闂?WebView/Electron 娑斿鈧绱濋崶鐘愁劃娑撳秷绺肩粔?Electron閿涙保anvas 濞撳弶鐓嬮崳銊﹀Ω閸忓鐖ｉ惄瀛樺复閻㈣婀悽璇茬娑撳﹥娓剁粙鍐蹭淮閵嗗倵鎲紾PU 瀵偓閸忓磭鏁?Rust 閺嶅洤绻旈弬鍥︽閿涘牓娼?localStorage閿涘绱濋崶鐘绘付閸?WebView2 閸掓稑缂撻崜宥堫嚢閵嗗倵鎲块崜宥囶伂闁挎瑨顕ゆ潪顒€褰傞崚鏉挎倱娑撯偓娴犺姤妫╄箛妤佹瀮娴犺绱濋崜宥呮倵缁旑垱妞傞梻瀵稿殠閸欘垰顕鎰┾偓?- **妤犲矁鐦夐敍?* `npx tsc --noEmit` PASS閿涙矖cargo check` PASS閿涙矖tauri:build` exit 0閿涙稑褰傜敮?URL閿涙ttps://gitee.com/argustang/myshell/releases/tag/v1.9.0

## 娴滄棃妫堕柌宥呮儙濡偓閺屻儻绱欓梼鑸殿唽 46閿?| 闂傤噣顣?| 缁涙梹顢?|
|------|------|
| 閹存垵婀崫顏堝櫡閿?| 闂冭埖顔?46 complete 閳ユ柡鈧?v1.9.0 瀹告彃褰傜敮鍐跨礉staging 瀹稿弶绔荤粚鎭掆偓涔seline閳姵1.9.0 |
| 閹存垼顩﹂崢璇叉憿闁插矉绱?| 娑撳绔存い瑙勬暭閸斻劌鐣幋鎰閹?doc-after-feature 鏉╄棄濮?staging 閺夛紕娲?|
| 閻╊喗鐖ｉ弰顖欑矆娑斿牞绱?| 缂佸牏顏崗澶嬬垼/闁灏崣顖濐潌閹勭壌閸ョ姳鎱ㄦ径?+ 瀵倸鐖堕崣顖濈槚閺傤叏绱欓弮銉ョ箶閸忋劑鎽肩捄顖濐洬閻╂牭绱?|
| 閹存垵顒熼崚棰佺啊娴犫偓娑斿牞绱?| xterm 濞撳弶鐓嬮崳銊┾偓澶嬪閺勵垰鍘滈弽鍥６妫版娈戦弽绋跨妇閿涘矂娼?webview 閺傝顢嶉敍姹玃U 缁備胶鏁ゆい璇叉躬 webview 閸掓稑缂撻崜宥堫啎閻滎垰顣ㄩ崣姗€鍣洪敍娑樺缁旑垶鏁婄拠顖濇祮閸欐垵鍩?Rust 閺冦儱绻旈弰顖濈槚閺傤厼澧犵粩顖氱磽鐢摜娈戦張澶嬫櫏閹靛顔?|
| 閹存垵浠涙禍鍡曠矆娑斿牞绱?| 閻楀牊婀?bump 1.9.0+sync閿涙保HANGELOG/README 閸?v1.9.0 閼哄偊绱盋anvas 濞撳弶鐓嬮崳?cursorStyle 娣囶喖顦?濞撳弶鐓嬮崳?GPU 瀵偓閸忕绱盧ust 7 濡€虫健+閸撳秶顏崗銊ョ湰閺冦儱绻旈敍娉僺c+cargo+tauri:build閿涙矞ommit+push閿涙鲍itee 閸欐垵绔烽敍娉倀aging 濞撳懐鈹栭敍娌礶mory 閸氬本顒?CLAUDE.md 缂佸繘鐛欓敍娌簉ogress 闂冭埖顔?6 |


### 闃舵 47锛氭棩蹇楄劚鏁?+ 宸︿笅瑙掑弽棣堝叆鍙ｏ紙2026-07-09锛?
- **闇€姹傦細** 瀹屽杽鏃ュ織杈撳嚭骞跺鏍稿績淇℃伅鑴辨晱锛涘乏涓嬭澧炲姞鍙嶉鍏ュ彛锛屽厑璁哥敤鎴锋彁浜ゅ弽棣堬紙濉啓鎻忚堪 + 娣诲姞鍥剧墖 + 鑷姩闄勪笂鑴辨晱鍚庣殑鏃ュ織锛夛紝閫氳繃 Web3Forms 鎻愪氦鍒版寚瀹氶偖绠便€?
- **鏋舵瀯锛堟牴鎹墠鏈熻璁鸿皟鏁达級锛?*
  - **鎻愪氦娓犻亾锛氶€夊畾 Web3Forms銆?* issue 鐩磋繛鐨?token 鎵撳寘杩?app 浼氭硠闇诧紝涓嶈兘鐢紱SMTP 鍑嵁瀵嗙爜鍚岄棶棰橈紝鎺掗櫎锛涘悗绔腑杞閮ㄧ讲锛屾帓闄や簡銆俉eb3Forms access_key 鍏紑瀹夊叏锛堝畼鏂规ā鍨嬶紝娉勬紡鏈€澶氳嚧鏀朵欢绠卞瀮鍦撅紝涓嶄細璐﹀彿琚洍锛夛紝閫変腑銆?
  - **鍥剧墖澶勭悊锛氬厤璐圭増涓嶆敮鎸侀檮浠讹紝鍥犳鍥剧墖鎵撳寘鎴愭湰鍦?zip锛堢敤 fflate锛夛紝鐢ㄦ埛鍙€夋墜鍔ㄩ檮甯︺€?
  - **Origin 椋庨櫓锛?* Tauri 妗岄潰绔?origin 鏈瀹樻柟璇佸疄锛屽厛瀹炵幇鍚庡疄娴嬶紱琚嫤鎴垯鐢ㄦ湰鍦?zip 鍏滃簳锛屽苟鎻愮ず鑱旂郴 Web3Forms 鏀捐 tauri.localhost銆?
- **瀹炵幇锛?*
  - **鏃ュ織鑴辨晱锛堟ā鍧椻憼锛夛細** 鏂板 redact.rs锛屾彁渚?host()銆乽ser()銆乻crub_log_text()銆傜瓥鐣ワ細IP 淇濈暀棣?灏炬锛屼腑闂存帺鐮侊紙渚嬪 192.168.1.10鈫?92.*.*.10锛夛紱涓绘満鍚?鐢ㄦ埛鍚嶄繚鐣欓灏惧瓧绗︼紱闀垮害鈮? 瀛楃鏁翠綋鎺╃爜涓?***銆?
  - **鑴辨晱瑙︾偣锛?* ssh.rs:272锛堝凡鐭ヤ富鏈哄湴鍧€鏃ュ織锛屽惈浠ｇ悊涓绘満+鐩爣涓绘満锛夈€乫tp.rs:49锛堣繛鎺ユ棩蹇楋紝鍚富鏈哄悕锛夈€乫tp.rs:75锛堢櫥褰曟棩蹇楋紝鍚敤鎴峰悕锛夈€乻sh.rs known_hosts 3 鏉℃棩蹇楋紙鍚富鏈哄悕锛夈€傚瘑鐮?瀵嗛挜鏈韩涓嶈褰曪紝缁存寔鐜扮姸銆?
  - **鏆撮湶鏃ュ織缁欏墠绔紙妯″潡鈶★級锛?* 鏂板 3 涓?tauri command锛歡et_feedback_log锛堣繑鍥炶劚鏁忓悗鐨勬棩蹇楋紝鍚綋澶?鏄ㄥぉ锛屾渶澶?200KB锛屽熬閮ㄦ埅鏂級銆乺eveal_path锛堟墦寮€鏃ュ織鐩綍锛岀櫧鍚嶅崟鏍￠獙锛夈€乻ave_feedback_zip锛堝皢鍓嶇 zip 瀛樺叆 feedback/ 瀛愮洰褰曪紝鏂囦欢鍚嶈繃婊わ級銆?
  - **鍙嶉鍏ュ彛锛堟ā鍧椻憿a锛夛細** Sidebar 鐗堟潈 footer 涓婃柟鍔?鍙嶉涓庡缓璁?琛岋紝闀滃儚鐜版湁 footer 妯″紡锛岀偣鍑绘墦寮€ FeedbackDialog銆?
  - **FeedbackDialog 缁勪欢锛堟ā鍧椻憿b锛夛細** 绫诲瀷锛堥棶棰樻姤鍛?鍔熻兘寤鸿/鍏朵粬锛? 鎻忚堪锛堝繀濉級+ 鑱旂郴鏂瑰紡锛堥€夊～锛? 鎴浘锛坓etDisplayMedia 鎴睆锛屼笉鍙敤鏃堕檷绾т负鏈湴閫夊浘锛? 鏈湴閫夊浘锛堢敤 plugin-dialog open锛? 鑷姩闄勪笂鏃ュ織锛堝彲灞曞紑鏌ョ湅/缂栬緫锛屽彲鍙栨秷锛夈€?
  - **鎻愪氦閫昏緫锛?* 濮嬬粓鍏堝瓨鏈湴 zip锛堝惈 feedback.txt + 鏃ュ織+ 鍥剧墖锛屼繚搴曢€氶亾锛夛紱鍐?fetch Web3Forms POST锛堟枃鏈? 鏃ュ織锛屽厤璐圭増鏃犻檮浠讹級锛涙垚鍔熷垯鎻愮ず鎵撳紑鏂囦欢澶癸紱澶辫触锛堝惈 403 Origin 鎷︽埅锛夋樉绀洪敊璇? 鏈湴 zip 鍏滃簳銆?
  - **App.tsx 鎺ョ嚎锛堟ā鍧椻憿c锛夛細** showFeedback state + Sidebar onOpenFeedback prop + 娓叉煋 FeedbackDialog銆?
- **瀹夊叏锛?* 鈶?瀵嗙爜鏈韩涓嶈褰曪紱鈶?host/user/proxy 鍦ㄧ偣閲?API 绂诲紑鍓嶈劚鏁忥紱鈶?get_feedback_log 璇诲嚭鍚庡啀璺?scrub_log_text 浜屾鑴辨晱锛堥槻浣忓巻鍙叉棩蹇?绗笁鏂瑰簱杈撳嚭锛夛紱鈶?reveal_path 鐧藉悕鍗曢檺鍒跺埌 myshell/logs锛堜笉 xml 鍏佽浠绘剰璺緞锛夛紱鈶?save_feedback_zip 鏂囦欢鍚嶈繃婊わ紙鍙?alphanum/-/_/.锛夈€?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛堝墠绔凡閫氳繃锛夛紱Rust 渚ф湭缂栬瘧锛堢幆澧冩棤 rustup锛夛紝寰?cargo check 楠岃瘉锛涘緟濉叆 Web3Forms access_key 鍚庡疄娴嬫彁浜ゃ€?
- **浼樺厛绾э細** 鈶犱负鏈€楂橈紝鐢ㄦ埛闇€瑕佸弽棣堥€氶亾锛涒憽蹇呴』瀹炴祴 Origin锛堜笉鐭ユ槸鍚︿笂绾匡級銆?
- **宸茬煡闄愬埗锛?* Web3Forms 瀹炴祴濡傛灉琚?403 鎷︽埅锛屽垯闇€ 鈶磋仈绯?Web3Forms 鏀寔鏀捐 tauri.localhost锛屾垨 鈶靛垏鎹㈠埌浼佷笟寰俊/閽夐拤 webhook锛堜絾鍥剧墖闇€棰濆鍥惧簥锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 47锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 47 complete 鈥斺€?鏃ュ織鑴辨晱+ 鍙嶉闈㈡澘宸插啓瀹岋紝tsc PASS锛汻ust 渚ф湭缂栬瘧锛堢幆澧冩棤 rustup锛夈€?|
| 鎴戣鍘诲摢閲岋紵 | 鈶?瀹夎 Rust 鍚?cargo check 楠岃瘉 Rust 渚э紱鈶?濉叆鐪熷疄 Web3Forms key 鍚庡疄娴嬫彁浜ゆ槸鍚﹀彈 Origin 闄愬埗锛涒懚 濡傛灉琚嫤鎴紝鑱旂郴鏀寔鎴栧垏鎹㈡柟妗堛€?|
| 鐩爣鏄粈涔堬紵 | 璁╃敤鎴疯兘鏂逛究鍦版姤鍛婇棶棰? 鎻愬缓璁紝鍚屾椂涓嶆硠闇?token 绮炬満瀵嗙爜/绉佹湁鎺ュ満鐮併€?|
| 鎴戝鍒颁簡浠€涔堬紵 | 璁捐鏃剁殑璁ㄨ锛氬紕娓?Gitee token 涓嶈兘鎵撳寘杩?app锛屾墍浠ユ斁寮?issue 鏂规锛涢€夌敤 Web3Forms锛堥偖绠憋紝access_key 鍏紑瀹夊叏锛夛紱鍥剧墖鐢ㄦ湰鍦?zip 鍥犺矾寰勭畝鍗曪紝浼氫负鐢ㄦ埛澶?redux 鍚屼竴缁勪欢锛堥暅鍍忕幇鏈?about/feedback 琛岋級銆?|
| 鎴戝仛浜嗕粈涔堬紵 | redact.rs 鑴辨晱妯″潡锛堜簩娆¤劚鏁忥級锛? 澶勬棩蹇楃偣+ 1 澶勮劚鏁忥紱get_feedback_log/ reveal_path/ save_feedback_zip 3 涓?command锛汧eedbackDialog 鍏ㄥ锛汼idebar 鍏ュ彛+ App.tsx 鎺ョ嚎锛沠flate 渚濊禆锛泃sc PASS銆?|

### 闃舵 48锛氬弽棣堟彁浜ゆ敼閫犱负 mailto + 淇锛?026-07-13锛?
- **闇€姹傦細** Web3Forms 鍦?Tauri 妗岄潰绔 403 鎷︽埅锛坥rigin 涓嶆敮鎸侊級锛岀涓夋柟鍥惧簥锛圛mgBB/Telegraph锛夋秹瀚岃繚娉曡灏佺銆傞渶瑕佸交搴曢噸鏋勫弽棣堟彁浜ゆ笭閬撱€?
- **鏂规婕旇繘锛?* 鈶?鍏堣€冭檻鑷缓 Cloudflare Worker + R2 + Resend锛圚MAC 绛惧悕銆侀€熺巼闄愬埗銆丮IME 鏍￠獙锛夛紝浠ｇ爜宸插啓瀹屼絾鐢ㄦ埛鏈€缁堟斁寮冿紙閮ㄧ讲杩愮淮鎴愭湰锛夛紱鈶?鏀逛负绠€娲佸彲闈犵殑 mailto 鏂规鈥斺€斿敜璧锋湰鍦伴偖浠跺鎴风锛屽弽棣堝唴瀹瑰鍒跺埌鍓创鏉裤€?
- **瀹炵幇锛?*
  - **Rust锛坢ain.rs锛夛細** `open_external_url` 鐧藉悕鍗曞鍔?`mailto:` 鏀寔锛涙柊澧?`clear_feedback_dir` 鍛戒护锛堝叧闂弽棣堢獥鍙ｆ椂娓呯┖ feedback/ 鐩綍锛夛紱`reveal_path` 鐧藉悕鍗曚粠 `logs/` 鎵╁睍鍒?`myshell/` 鐖剁洰褰曪紙淇鎵撳紑 feedback 鏂囦欢澶规棤鏁堬級锛涙柊澧?`try_focus_existing_explorer`锛圵in32 EnumWindows 鏌ユ壘宸叉墦寮€鐨勫悓鍚嶈祫婧愮鐞嗗櫒绐楀彛锛屾壘鍒板垯婵€娲诲墠缃紝涓嶉噸澶嶆墦寮€锛夈€?
  - **鍓嶇锛團eedbackDialog.tsx锛夛細** 鍒犻櫎 Web3Forms/Telegraph 鍏ㄥ浠ｇ爜锛沗handleSubmit` 鏀逛负锛氫繚瀛樻湰鍦?zip 鈫?鍓创鏉垮鍒跺弽棣堝唴瀹?鈫?`openExternalUrl(mailto:...)` 鍞よ捣閭欢瀹㈡埛绔紙鍙甫涓婚锛孶RL 鐭笉涓級鈫?`window.alert()` 寮圭獥鎻愰啋鐢ㄦ埛 Ctrl+V 绮樿创 + 鎷栭檮浠躲€?
  - **鍓嶇锛坅pi.ts锛夛細** 鍒犻櫎 `uploadScreenshot`/`submitFeedback`/`FeedbackImage`锛涙柊澧?`clearFeedbackDir` wrapper銆?
  - **鏂板 Gitee Issue 鍏ュ彛锛?* 鍙嶉绐楀彛 footer 宸︿晶鍔犻摼鎺ワ紝鐐瑰嚮鎵撳紑 `gitee.com/argustang/myshell/issues/new`銆?
  - **澶辫触 UI 閲嶈璁★細** 鎻愪氦澶辫触鏃?鉂?绾㈣壊閱掔洰鎻愮ず锛屼笌鎴愬姛 鉁?缁胯壊鏄庢樉鍖哄垎銆?
  - **澶辫触鏃ュ織锛?* `handleSubmit` 鎵€鏈?catch 鍧楀鍔?`writeFrontendLog`锛岀綉缁滈敊璇啓鍏ュ簲鐢ㄦ棩蹇椼€?
- **瀹夊叏锛?* 鍒犻櫎浜嗙紪璇戣繘浜岃繘鍒剁殑 IMGBB_API_KEY锛堝浘搴婂凡灏佺锛夛紱mailto 涓嶇粡杩囦换浣曠涓夋柟鏈嶅姟銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛沗cargo check` PASS锛況elease 鎵撳寘鎴愬姛銆?
- **宸茬煡闄愬埗锛?* mailto 鏃犳硶鑷姩娣诲姞闄勪欢锛岄渶鐢ㄦ埛鎵嬪姩鎷?zip 杩涢偖浠讹紱閭欢瀹㈡埛绔€夋嫨鍣ㄤ笌璧勬簮绠＄悊鍣ㄧ獥鍙ｅ瓨鍦ㄧ劍鐐圭珵浜夛紙宸查€氳繃"涓嶈嚜鍔ㄦ墦寮€鏂囦欢澶?瑙勯伩锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 48锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 48 complete 鈥斺€?鍙嶉鎻愪氦鏀归€犱负 mailto 鏂规锛屽凡鍙戝竷 v1.10.2銆?|
| 鎴戣鍘诲摢閲岋紵 | 涓嬩竴涓姛鑳芥垨鐢ㄦ埛鍙嶉鐨勬柊闇€姹傘€?|
| 鐩爣鏄粈涔堬紵 | 璁╃敤鎴疯兘鍙潬鍦版彁浜ゅ弽棣堬紝涓嶄緷璧栦笉绋冲畾鐨勭涓夋柟鏈嶅姟銆?|
| 鎴戝鍒颁簡浠€涔堬紵 | mailto body 鍦?QQ 閭绛夊鎴风浼氳妯℃澘鏇挎崲/涓㈠純锛屼笉鑳戒緷璧?body 鍙傛暟浼犲唴瀹癸紱鍓创鏉挎槸 100% 鍙潬鐨勬浛浠ｏ紱explorer 鍜?mailto 绐楀彛鏈夌劍鐐圭珵浜夛紝涓嶈兘鍚屾椂鑷姩寮瑰嚭銆?|
| 鎴戝仛浜嗕粈涔堬紵 | main.rs锛坢ailto 鐧藉悕鍗曘€乧lear_feedback_dir銆乺eveal_path 鐧藉悕鍗曟墿灞曘€乪xplorer 鍘婚噸锛夛紱api.ts锛堝垹鏃?wrapper 鍔犳柊锛夛紱FeedbackDialog.tsx锛坢ailto 娴佺▼銆佸壀璐存澘銆佸脊绐楁彁閱掋€丟itee 閾炬帴銆佸け璐ョ孩鑹?UI锛夛紱鍙戝竷 v1.10.2 鍒?Gitee銆?|

### 闃舵 49锛氬彂甯?v1.11.0 鈥斺€?AI 渚涘簲鍟嗗乏鍙冲竷灞€ + 澶氭ā鍨嬬鐞?+ 涓ゆ閫夋嫨锛?026-07-14锛?
- **闇€姹傦細** 璁剧疆鈫扐I 鍔╂墜鐣岄潰鏀逛负宸﹀彸甯冨眬锛堝乏渚т緵搴斿晢鍒楄〃锛屽彸渚ц鎯咃級锛汚I 鑱婂ぉ绐楀彛妯″瀷閫夋嫨鏀逛负涓ゆ锛堝厛閫変緵搴斿晢鍐嶉€夋ā鍨嬶級锛涗粎灞曠ず鐢ㄦ埛鑷畾涔夌殑宸插惎鐢ㄤ緵搴斿晢锛岄璁句笉鍐嶆贩鍏ャ€?
- **鏁版嵁妯″瀷鍙樻洿锛?* 鏂板 ai_supplier_models 琛紙姣忎緵搴斿晢 N 涓ā鍨嬶級锛涙柊澧?ai_settings.active_model_string 鍒楋紱鏂板 ai_models.is_enabled 鍒椼€?
- **鍚庣锛坅i.rs锛夛細** AiModelInfo 澧炲姞 models + is_enabled锛? 涓柊鍛戒护锛坙ist/add/remove supplier_models + toggle_enabled锛夛紱fetch_models_for_supplier 鏈嶅姟绔В瀵?key锛泃est_settings 鏂板 supplier_id 鍙傛暟锛泂ave_ai_model_cmd 鏀寔 models 鍚屾锛泂et_active_ai_model_cmd 鏀寔 model_string銆?
- **鍓嶇锛?* SettingsPanel AI 鏍囩椤靛乏鍙冲竷灞€閲嶅啓锛堝乏渚у垪琛?寮€鍏筹紝鍙充晶璇︽儏+妯″瀷绠＄悊+Toast+鍒涘缓鏍￠獙锛夛紱AiPanel 涓ゆ绾ц仈閫夋嫨鍣紙杩囨护棰勮锛夛紱api.ts 鏂板 SupplierModel + 5 涓嚱鏁帮紱capabilities/main.json 鏂板 dialog 鏉冮檺銆?
- **鐗堟湰鍙凤細** 鍚?3 椤规柊澧?鈫?minor锛歷1.10.2 鈫?v1.11.0銆?
- **楠岃瘉锛?* tsc PASS锛沜argo check PASS锛泃auri:build exit 0锛涘彂甯?URL锛歨ttps://gitee.com/argustang/myshell/releases/tag/v1.11.0

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 49锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 49 complete 鈥斺€?v1.11.0 宸插彂甯冨埌 Gitee锛宻taging 宸叉竻绌恒€乥aseline鈫抳1.11.0 |
| 鎴戣鍘诲摢閲岋紵 | 涓嬩竴椤规敼鍔ㄥ畬鎴愭椂鎸?doc-after-feature 杩藉姞 staging 鏉＄洰 |
| 鐩爣鏄粈涔堬紵 | AI 渚涘簲鍟嗛厤缃綋楠岄噸鏋勶細宸﹀彸甯冨眬娓呮櫚銆佸妯″瀷绠＄悊鐏垫椿銆佽亰澶╃獥鍙ｄ袱姝ラ€夋嫨涓嶆贩涔?|
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri v2 涓?window.alert 鏄犲皠鍒?dialog.message 闇€鍦?capabilities 鏄惧紡鎺堟潈锛涗繚瀛樺悗 editKey 娓呯┖瀵艰嚧鑾峰彇妯″瀷澶辫触锛岃В娉曟槸鏂板 fetch_models_for_supplier 鏈嶅姟绔В瀵嗭紱娴嬭瘯闇€浼?supplier_id 鍚﹀垯娴嬪叏灞€娲昏穬渚涘簲鍟?|
| 鎴戝仛浜嗕粈涔堬紵 | db.rs + ai.rs + main.rs锛堟暟鎹ā鍨?+ 4 鍛戒护 + test 淇锛夛紱api.ts + SettingsPanel + AiPanel锛堝墠绔噸鍐欙級锛沜apabilities dialog 鏉冮檺锛涘彂甯?v1.11.0 鍒?Gitee |

### 闃舵 50锛氬崌绾у畨瑁呪€斺€旈潤榛樺嵏杞芥棫鐗堬紙NSIS 妯℃澘鑷畾涔夛級锛?026-07-15锛?
- **闇€姹傦細** 鐢ㄦ埛鍙嶉姣忔鍗囩骇鐗堟湰瑕?鍏堢偣涓€娆″嵏杞姐€佸啀鐐逛竴娆″畨瑁?寰堥夯鐑︼紝甯屾湜鍚堝苟鎴愮偣涓€娆°€?
- **鏍瑰洜锛堝畼鏂规簮鐮佸疄璇侊級锛?* Tauri 榛樿 NSIS 妯℃澘 `installer.nsi` 鐨?`PageLeaveReinstall` 鈫?`reinst_uninstall` 鍧楋紝璋冪敤鏃х増 `uninstall.exe` 鏃跺彧杩藉姞 `_?=$4`锛堥潪闈欓粯锛夛紝鍙屽嚮鏂板畨瑁呭寘锛堟櫘閫氭ā寮忥級鏃舵棫鐗堝嵏杞藉櫒寮瑰嚭瀹屾暣鍗歌浇鍚戝鐣岄潰锛堢‘璁ら〉+杩涘害椤碉級锛岄€犳垚"鍐嶅嵏杞戒竴杞?鐨勪綋楠屻€俙installerHooks` 澶熶笉鐫€杩欒锛堥潪瀹忔彃鍏ョ偣锛夛紝蹇呴』鐢?`nsis.template` 鎺ョ鏁翠唤妯℃澘銆?
- **鏂规锛?* 鏂板缓 `src-tauri/nsis/installer.nsi`锛?瀹樻柟 tauri-v2.11.2 妯℃澘鍘熸枃 + 鏂囦欢澶存敞閲婏級锛屽敮涓€鏀瑰姩锛歚reinst_uninstall` 鐨?`StrCpy "$R1 _?=$4"` 鏀逛负 `"$R1 /S _?=$4"`锛堝姞 `/S` 闈欓粯锛夈€俙tauri.conf.json` 鍔?`"template": "nsis/installer.nsi"`銆?
- **鏀瑰姩鍚庝綋楠岋細** 鍙屽嚮鏂板寘 鈫?娆㈣繋椤?鈫?PageReinstall 椤碉紙榛樿閫変腑"鍗歌浇鍚庡畨瑁?锛岀偣涓€涓嬩笅涓€姝ワ級鈫?鏃х増**鍚庡彴闈欓粯鍗歌浇锛堟棤鐣岄潰锛?* 鈫?瑁呮柊鐗?鈫?瀹屾垚銆備粠 3 鍏充氦浜掗檷鍒?1 娆?涓嬩竴姝?銆?
- **杈圭晫鎯呭喌锛堝凡纭瀹夊叏锛夛細** 鏃犳棫鐗堟椂 `reinst_uninstall` 涓嶆墽琛岋紱鏃х増鍦ㄨ繍琛屾椂闈欓粯鍗歌浇鍣ㄨ嚜鍔?kill 杩涚▼锛涘嵏杞藉け璐ラ€€鍑虹爜妫€鏌ヤ繚鐣欎笉鍙橈紱`/S` 涓嬩笉鏄剧ず"鍒犻櫎搴旂敤鏁版嵁"鍕鹃€夋锛堝崌绾ф湡鏈涜涓猴紝涓嶅垹鏁版嵁锛夈€?
- **缁存姢鎴愭湰锛?* 鎺ョ妯℃澘鍚庯紝浠婂悗 Tauri 鍗囩骇鑻ユ敼浜?`installer.nsi`锛岄渶 diff 鏂板畼鏂规ā鏉夸笌鏈壇鏈紝鎶?`/S` 鏀瑰姩閲嶆柊搴旂敤涓婂幓锛堟枃浠跺ご娉ㄩ噴宸插啓鏄庯級銆?
- **楠岃瘉锛?* `npm run tauri:build` exit 0锛沵akensis 鎴愬姛缂栬瘧鑷畾涔夋ā鏉匡紝鐢熸垚 `MyShell_1.11.0_x64-setup.exe`锛坄/S` 鏀瑰姩鏈牬鍧?NSIS 璇硶锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 50锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 50 complete 鈥斺€?鑷畾涔?NSIS 妯℃澘鍔?`/S` 闈欓粯鏃х増鍗歌浇锛宼auri:build 閫氳繃銆?|
| 鎴戣鍘诲摢閲岋紵 | 涓嬫鍙戝竷鏃讹紙`鎵撳寘`锛夐殢鐗堟湰涓€璧蜂笂绾匡紱娉ㄦ剰杩欐槸 installMode perMachine 涓嬬殑鏀瑰姩锛岃嫢灏嗘潵鍒?installMode 闇€澶嶆牳銆?|
| 鐩爣鏄粈涔堬紵 | 鍗囩骇鏃跺弻鍑讳竴娆″畨瑁呭寘鍗宠嚜鍔ㄥ嵏杞芥棫鐗堝苟瑁呮柊鐗堬紝鏃х増鍗歌浇姝ラ涓嶅啀寮瑰悜瀵肩晫闈€?|
| 鎴戝鍒颁簡浠€涔堬紵 | Tauri NSIS 鐨?`installerHooks`锛堝畯娉ㄥ叆锛夊涓嶇潃 `PageLeaveReinstall` 閲岀殑 `ExecWait` 璋冪敤锛涜鏀瑰嵏杞藉櫒璋冪敤鍙傛暟蹇呴』鐢?`nsis.template` 鎺ョ鏁翠唤妯℃澘锛沗/S` 鏄?NSIS 鍗歌浇鍣ㄩ潤榛樻爣蹇楋紙澶у啓锛夛紝閰嶅悎 `_?=dir` 鎵嶈兘鍦ㄥ師浣嶆墽琛屻€?|
| 鎴戝仛浜嗕粈涔堬紵 | 鏂板缓 `src-tauri/nsis/installer.nsi`锛堝畼鏂规ā鏉?+ reinst_uninstall 鍔?`/S` + 鏂囦欢澶存敞閲婏級锛沗tauri.conf.json` 鍔?nsis.template 瀛楁锛泃auri:build 楠岃瘉閫氳繃锛沺rogress/staging/README 鏂囨。鍚屾銆?|

### 闃舵 51锛氫慨澶?SSH 闀垮懡浠ら潤榛樻湡琚鏉€锛坕nactivity_timeout 鈫?keepalive锛夛紙2026-07-15锛?
- **鐜拌薄锛?* 鐢ㄦ埛杩炴帴鏈嶅姟鍣ㄥ垏 root 鍚庢墽琛?`find . -maxdepth 1 -type f -printf "%TY-%Tm-%Td\n" | sort | uniq -c`锛屽懡浠よ繕娌¤窇瀹屽氨 `[Connection closed]`銆傚悓涓€鍙版湇鍔″櫒鐢?MobaXterm 涓嶉€€鍑恒€?
- **鏍瑰洜锛坮ussh 0.50.4 婧愮爜瀹炶瘉锛夛細** `ssh.rs:284` 璁句簡 `inactivity_timeout = 30s`銆俽ussh 鐨?inactivity timer 鏄?*杩炴帴绾?*闈欓粯瓒呮椂锛坄client/mod.rs:1033-1036`锛夛紝30s 鍐呮病鏀跺埌浠讳綍鏁版嵁鍖呭氨杩斿洖 `InactivityTimeout` 閿欒鏉€杩炴帴銆俙find | sort | uniq -c` 杩欑绠￠亾鍛戒护鍦?sort 闃舵锛堥樆濉炲紡娑堣垂 stdin锛夐暱鏃堕棿涓嶅悜 PTY 杈撳嚭浠讳綍瀛楄妭 鈫?30s 瑙﹀彂 鈫?`channel.wait()` 杩斿洖 `None` 鈫?`ssh_closed`銆傛棩蹇楀嵃璇侊細涓ゅ彴鏈嶅姟鍣ㄩ兘鍦ㄦ墽琛岄暱鍛戒护鍚?`channel.wait returned None`锛岀浉闅旂害 77s锛堝惈鍒?root + 鍛戒护闈欓粯鎵ц绱锛夈€?
- **涓轰粈涔?MobaXterm 涓嶅彈褰卞搷锛?* OpenSSH 绯讳笉璁?inactivity 瓒呮椂锛岄潬 keepalive 蹇冭烦缁存寔杩炴帴銆?
- **淇锛?* 鍘绘帀 `inactivity_timeout`锛屾敼鐢?`keepalive_interval = 15s` + `keepalive_max = 3`锛堣繛缁?3 娆℃棤鍝嶅簲 ~45s 鎵嶅垽瀹氭浜★級銆傞暱鍛戒护闈欓粯鏈?keepalive 蹇冭烦缁存寔杩炴帴锛涚湡鎸傛鐨勬湇鍔″櫒浠嶈兘鍦?~45s 鍐呮娴嬪埌銆?
- **楠岃瘉锛?* `cargo check` PASS锛沝ev 鍚姩姝ｅ父銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 51锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 51 complete 鈥斺€?keepalive 鏇夸唬 inactivity_timeout锛宑argo check 閫氳繃锛屽緟鐢ㄦ埛瀹炴祴闀垮懡浠や笉鍐嶆柇寮€銆?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛瀹炴祴楠岃瘉锛涚‘璁ゅ悗闅忎笅娆″彂甯冧笂绾裤€?|
| 鐩爣鏄粈涔堬紵 | 浜や簰寮?shell 璺戦暱鍛戒护锛坒ind\|sort銆乼ar銆佺紪璇戠瓑锛変笉鍐嶈璇潃锛屽悓鏃朵粛鑳芥娴嬬湡鎸傛鐨勬湇鍔″櫒銆?|
| 鎴戝鍒颁簡浠€涔堬紵 | russh 鐨?inactivity_timeout 鏄繛鎺ョ骇 idle timer锛堟棤鏁版嵁鍖呭嵆鏉€锛夛紝涓嶉€傚悎浜や簰寮忕粓绔€斺€旈暱鍛戒护鏈夊悎娉曢暱闈欓粯鏈燂紱keepalive 鎵嶆槸姝ｉ亾锛堝績璺崇淮鎸?鏃犲搷搴旀墠鏂級锛屼笌 OpenSSH/MobaXterm 鍚屾€濊矾銆傜敤鎴锋湡鏈涚殑"闄嶇骇鍒?ias 鍐嶉€€鍑?鍦ㄥ綋鍓嶆灦鏋勪笉瀛樺湪锛坰u 鍒?root 鍙槸 PTY 鍐呭懡浠わ紝SSH 浼氳瘽灞傞潰鏄悓涓€鏉¤繛鎺ワ級銆?|
| 鎴戝仛浜嗕粈涔堬紵 | `ssh.rs` 鎶?`inactivity_timeout=30s` 鎹㈡垚 `keepalive_interval=15s`+`keepalive_max=3`锛屾洿鏂版敞閲婅鏄庢牴鍥犲拰淇鐞嗙敱銆?|

### 闃舵 52锛氬尶鍚嶇増鏈粺璁℃敼涓烘瘡娆″崌绾ч兘寮圭獥璇㈤棶锛?026-07-15锛?
- **闇€姹傦細** 鐢ㄦ埛甯屾湜姣忔鐗堟湰鍗囩骇瀹屾垚鍚庨兘寮瑰嚭鍖垮悕缁熻鍚屾剰鎻愮ず锛岃€屼笉鏄?棣栨鍚屾剰鍚庡悗缁増鏈潤榛樹笂鎶?銆?
- **鐜扮姸锛?* `usageStats.ts` 鐨?`checkReportNeeded` 瀵规湭涓婃姤鐗堟湰浼氭煡 `hasStatsConsent()`鈥斺€旇嫢涔嬪墠鍚屾剰杩囧垯闈欓粯涓婃姤涓嶅脊绐椼€?
- **淇锛?* `checkReportNeeded` 璁?`hasConsent` 姘歌繙杩斿洖 false锛孉pp.tsx 姘歌繙璧板脊绐楀垎鏀€俙setStatsConsent` 浠嶈褰曞亸濂斤紙浠ュ鍥為€€锛夈€傚脊绐楁枃妗堜粠"鍚屾剰鍚庡皢璁颁綇鍋忓ソ锛屽悗缁増鏈崌绾ц嚜鍔ㄧ粺璁′笉鍐嶈闂?鏀逛负"姣忔鍗囩骇鍒版柊鐗堟湰閮戒細璇㈤棶涓€娆?銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 52锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 52 complete 鈥斺€?缁熻寮圭獥鏀逛负姣忔鐗堟湰鍗囩骇閮借闂紝tsc 閫氳繃锛宒ev 閲嶅惎涓€?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛瀹炴祴纭寮圭獥琛屼负锛涚‘璁ゅ悗闅忎笅娆″彂甯冧笂绾裤€?|
| 鐩爣鏄粈涔堬紵 | 姣忔鐗堟湰鍗囩骇棣栨鍚姩閮藉緛姹傜敤鎴峰悓鎰忥紝灏婇噸鐢ㄦ埛瀵规瘡娆℃暟鎹笂鎶ョ殑鐭ユ儏閫夋嫨鏉冦€?|
| 鎴戝鍒颁簡浠€涔堬紵 | 鏀瑰姩鏈€灏忓寲鍘熷垯锛氬彧鏀?`checkReportNeeded` 杩斿洖鍊硷紙hasConsent 鎭?false锛夛紝App.tsx 鐨勫垎鏀€昏緫瀹屽叏涓嶇敤鍔紱淇濈暀 `setStatsConsent` 璋冪敤浠ュ鍥為€€銆?|
| 鎴戝仛浜嗕粈涔堬紵 | `usageStats.ts` checkReportNeeded 鏀逛负鎭掑脊绐楋紱`StatsConsentDialog.tsx` 鏂囨鏇存柊锛泃sc PASS銆?|

### 闃舵 53锛氫慨澶嶅尶鍚嶇粺璁″脊绐楀悓鐗堟湰姣忔鍚姩閮介噸澶嶅脊鍑猴紙2026-07-17锛?
- **鐜拌薄锛?* 姣忔鍚姩 MyShell 閮藉脊鍑哄尶鍚嶇粺璁″悓鎰忓脊绐楋紝鑰屼笉鏄鏈熺殑"姣忔鐗堟湰鍗囩骇鍚庣殑棣栨鍚姩鎵嶅脊"銆?
- **鏍瑰洜锛?* 寮圭獥瑙﹀彂閫昏緫浠?`localStorage["myshell.statsVersion"]` 鍒ゆ柇褰撳墠鐗堟湰鏄惁宸插鐞嗚繃銆傝 key 鍙湪 `reportVersion()` 缃戠粶涓婃姤**鎴愬姛鍚?*鎵嶅啓鍏ワ紙`usageStats.ts:96`锛夈€傚綋鐢ㄦ埛鐐?鏆備笉"锛坉ecline锛夋椂锛屽彧璋冪敤浜?`setStatsConsent(false)` 绉婚櫎 consent key锛?*娌℃湁鍐欏叆 statsVersion**锛涘綋鐢ㄦ埛鐐?鍏佽"浣嗙綉缁滃け璐ユ椂锛屽悓鏍蜂笉浼氬啓鍏ャ€傛墍浠ヤ笅娆″惎鍔?`isVersionReported(version)` 浠嶄负 false锛屽脊绐楀啀娆″嚭鐜般€?
- **淇锛?* 鏂板 `markVersionHandled(version)` 鍑芥暟锛屽湪 `onAgree` 鍜?`onDecline` 涓や釜鍥炶皟涓兘璋冪敤锛岀‘淇濈敤鎴峰仛鍑哄喅瀹氬悗绔嬪嵆鏍囪褰撳墠鐗堟湰宸插鐞嗐€俙reportVersion` 鍐呴儴鍘熸湁鐨?`localStorage.setItem(KEY_VERSION, ...)` 淇濈暀锛堝悓鎰忔椂鍐椾綑鍐欏叆锛屾棤瀹充笖鍚戝悗鍏煎锛夈€傛洿鏂颁簡 `isVersionReported` 鍜?`checkReportNeeded` 鐨勬敞閲婏紝璇存槑 KEY_VERSION 鐨勮涔夋槸"宸插鐞?锛堝凡璇㈤棶骞跺凡鍐冲畾锛夛紝鑰屼笉鏄?宸蹭笂鎶?銆?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 53锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 53 complete 鈥斺€?鍖垮悕缁熻寮圭獥鍚岀増鏈噸澶嶅脊鍑?bug 宸蹭慨澶嶏紝tsc 閫氳繃銆?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛瀹炴祴纭鍚岀増鏈浜屾鍚姩涓嶅啀寮圭獥锛涢殢涓嬫鍙戝竷涓婄嚎銆?|
| 鐩爣鏄粈涔堬紵 | 鍚屼竴鐗堟湰涓嬪彧寮逛竴娆＄粺璁″悓鎰忓脊绐楋紙鏃犺鐢ㄦ埛鍚屾剰鎴栨嫆缁濓級锛岀増鏈彿鍙樺寲鎵嶉噸鏂板脊鍑恒€?|
| 鎴戝鍒颁簡浠€涔堬紵 | 鍖哄垎"宸蹭笂鎶?鍜?宸插喅瀹?涓や釜璇箟锛歚statsVersion` 鍘熸湰鍙湪涓婃姤鎴愬姛鍚庡啓锛屾紡鎺変簡"鐢ㄦ埛鎷掔粷"鍜?缃戠粶澶辫触"涓ょ鍦烘櫙銆備慨澶嶆€濊矾鏄妸"鏍囪鐗堟湰宸插鐞?鐨勮矗浠荤Щ鍒扮敤鎴峰仛鍑哄喅瀹氱殑鍥炶皟閲岋紙agree/decline锛夛紝鑰屼笉鏄斁鍦ㄧ綉缁滆姹傛垚鍔熶箣鍚庛€?|
| 鎴戝仛浜嗕粈涔堬紵 | `usageStats.ts` 鏂板 `markVersionHandled(version)` 骞舵洿鏂扮浉鍏虫敞閲婏紱`App.tsx` 鍦?onAgree/onDecline 涓皟鐢紱tsc PASS銆?|

### 闃舵 54锛歀inux deb 鎵撳寘鏀寔 + setup_file_logging 璺ㄥ钩鍙板畧鍗慨澶嶏紙2026-07-18锛?
- **鐩爣锛?* 涓?MyShell 澧炲姞 Linux .deb 浜х墿锛岃 Ubuntu/Debian 鐢ㄦ埛鑳界洿鎺?`dpkg -i` 瀹夎銆傛湰娆″彧楠岃瘉鎵撳寘娴佺▼鑳借窇閫氾紝**涓嶈繘鍙戝竷娴佹按绾?*锛堢増鏈繚鎸?1.11.2锛屼笉鎺?Gitee锛屼笉鏀?CHANGELOG锛夈€?
- **`tauri.conf.json` 鏀瑰姩锛?*
  - `bundle.targets`锛歚["nsis"]` 鈫?`["nsis", "deb"]`銆俉indows 涓婅窇 `tauri:build` 浠嶅彧浜?nsis锛圱auri 鎸夊涓?OS 鑷姩璺宠繃涓嶅吋瀹圭洰鏍囷級锛孡inux 涓婂悓鍛戒护鐩存帴浜?deb銆備竴娆￠厤缃€佽法骞冲彴閫氱敤銆?
  - 鏂板 `bundle.linux.deb.depends`锛氬０鏄?4 涓繍琛屾椂渚濊禆鈥斺€擿libwebkit2gtk-4.1-1`锛圱auri v2 Linux WebView 蹇呴渶锛夈€乣libgtk-3-0`锛堢獥鍙?鎺т欢锛夈€乣libsecret-1-0`锛坄keyring` crate 鐨?`sync-secret-service` feature 鐢級銆乣libayatana-appindicator3-1`锛堟墭鐩樺浘鏍囷級銆俛pt 瀹夎 deb 鏃朵細鑷姩鎷夎繖浜涘簱銆?
- **`src-tauri/src/main.rs` 鏀瑰姩锛堜慨澶嶈法骞冲彴缂栬瘧锛夛細** `setup_file_logging` 鍘熷睘鎬ф槸 `#[cfg(not(debug_assertions))]`锛屼絾鍑芥暟浣撳唴閮ㄧ敤鐨勬槸 MSVC CRT 涓撳睘 API锛坄_open_osfhandle` / `_dup2` / `std::os::windows::io::AsRawHandle`锛夈€俽elease build 鍦?Linux 涓婄紪璇戞椂鐐?`error[E0433]: cannot find 'windows' in os` 鍜?`error[E0599]: no method named 'as_raw_handle'`銆備慨澶嶏細
  - 鍘熷嚱鏁板睘鎬ф敼涓?`#[cfg(all(not(debug_assertions), windows))]`鈥斺€擶indows release 浠嶈蛋 CRT 閲嶅畾鍚?stderr 鍒版棩蹇楁枃浠躲€?
  - 鏂板 `#[cfg(all(not(debug_assertions), unix))]` 鐗堟湰锛?*绌?stub**銆傜悊鐢憋細Linux 涓?stderr 榛樿灏遍檮鍔犲埌鍚姩杩涚▼锛坉esktop launcher / shell锛夛紝鏃ュ織閲嶅畾鍚戜笉鏄?deb 鑳借窇璧锋潵鐨勫繀瑕佹潯浠讹紱涔嬪墠灏濊瘯鐢?`freopen` + `__stderr_location` 瀹炵幇 Unix 绛変环鐗堟湰锛屼絾 `__stderr_location` 鏄?glibc 涓撳睘绗﹀彿锛坢usl 涓婃病鏈夛級锛屼緷璧?FFI 鍙嶈€屽鍔犻闄┿€侺inux 鏃ュ織鏀寔寤跺悗鍒板悗缁増鏈紝鏈浼樺厛淇濊瘉 deb 鑳戒骇鍑恒€?
  - 鏂板 `#[cfg(debug_assertions)]` 鐨?debug 绌?stub鈥斺€攄ebug build 鏈潵灏变笉閲嶅畾鍚?stderr锛圵indows/Unix 閮戒笉锛夛紝浣嗗師鏉ラ潬 `#[cfg(not(debug_assertions))]` 闅愬紡璺宠繃锛涚幇鍦ㄦ媶鎴?3 涓?cfg 鍒嗘敮鍚庨渶瑕佹樉寮忚ˉ涓?debug stub锛屽惁鍒?debug build 涓?`setup_file_logging()` 璋冪敤鐐逛細鎵句笉鍒板嚱鏁般€?
- **鏈満鐜鍑嗗锛堜粠闆舵惌锛夛細** 杩欏彴 Ubuntu 26.04 寮€鍙戞満鍘熸湰瀹屽叏娌℃湁 Rust 鍜?Tauri Linux 鏋勫缓渚濊禆銆傜敤鎴锋巿鏉冨悗璺戜簡锛?
  - `sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libgtk-3-dev librsvg2-dev libglib2.0-dev libsoup-3.0-dev libsecret-1-dev libayatana-appindicator3-dev libdbus-1-dev rustup`
  - `rustup default stable`锛堣 rustc 1.97.1 + cargo 1.97.1锛?
  - `npm install`锛堣 `@tauri-apps/cli` 鍜屽墠绔緷璧栵紱esbuild postinstall 琚?npm allow-scripts 鎷︽埅浣嗕簩杩涘埗宸插氨浣嶏紝涓嶅奖鍝?build锛?
- **楠岃瘉锛?*
  - `npx tsc --noEmit` PASS
  - `cargo check --release` PASS锛堝彧鍓╀竴涓棤鍏崇殑 `is_openai_protocol` dead_code warning锛?
  - `npm run tauri:build` exit 0锛屼骇鍑?`src-tauri/target/release/bundle/deb/MyShell_1.11.2_amd64.deb`锛?.7 MB锛宨nstalled-size 29 MB锛?
  - `dpkg-deb -I` 楠岃瘉锛歚Package: my-shell`銆乣Version: 1.11.2`銆乣Architecture: amd64`銆乣Depends:` 鍚垜浠０鏄庣殑 4 涓簱锛圱auri 杩樿嚜鍔ㄨ拷鍔犲畠妫€娴嬪埌鐨?`libwebkit2gtk-4.1-0`銆乣libgtk-3-0`锛?
  - `dpkg-deb -c` 楠岃瘉锛氬竷灞€姝ｇ‘鈥斺€擿usr/bin/myshell`锛堜富绋嬪簭锛夈€乣usr/share/applications/MyShell.desktop`锛堟闈㈠叆鍙ｏ級銆乣usr/share/icons/hicolor/{32,128,256,512}x*/apps/myshell.png`锛坔icolor 鍥炬爣闆嗭級
- **宸茬煡閬楃暀锛?* Linux 涓?release build 娌℃湁 stderr 閲嶅畾鍚戝埌鏃ュ織鏂囦欢锛圲nix stub 鏄┖鐨勶級銆俉indows 琛屼负瀹屽叏涓嶅彉銆傚鏋滃皢鏉?Linux 涔熼渶瑕佽惤鐩樻棩蹇楋紝鍐嶅疄鐜?Unix 鐗堟湰锛堢敤 `freopen` 鑰岄潪 glibc 涓撳睘绗﹀彿锛夈€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 54锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 54 complete 鈥斺€?Linux deb 鎵撳寘娴佺▼璺戦€氾紝`MyShell_1.11.2_amd64.deb` 浜х墿楠岃瘉閫氳繃锛屽緟鐢ㄦ埛瀹炴祴瀹夎 + 杩愯銆?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛鍦?Ubuntu/Debian 涓?`sudo dpkg -i MyShell_1.11.2_amd64.deb` 瀹炴祴瀹夎涓庤繍琛岋紱纭鏃犺鍚庨殢涓嬫鍙戝竷锛坄鎵撳寘`锛夋寮忎笂绾?Linux 浜х墿銆?|
| 鐩爣鏄粈涔堬紵 | MyShell 鑳戒骇鍑哄彲鐩存帴 `dpkg -i` 瀹夎鐨?.deb 鍖咃紝apt 鑷姩鎷夎繍琛屾椂渚濊禆锛屽畨瑁呭悗寮€濮嬭彍鍗?妗岄潰鍥炬爣榻愬叏銆?|
| 鎴戝鍒颁簡浠€涔堬紵 | (1) 鍘熶粨搴撳彧閽堝 Windows锛宍setup_file_logging` 鐢ㄤ簡 MSVC CRT 涓撳睘 API 浣嗗彧鎸備簡 `cfg(not(debug_assertions))` 娌?`cfg(windows)`锛屾槸鍏稿瀷鐨?鍙湪 Windows 涓婃祴杩?鐨勫潙銆?2) Ubuntu 26.04 鐨?rustup 鍖呮妸 binary 鏀?`/usr/bin/`銆乼oolchain 鏀?`~/.rustup/`锛岃窡瀹樻柟 rustup 瑁呭埌 `~/.cargo/bin/` 涓嶄竴鏍凤紝浣嗗姛鑳界瓑浠枫€?3) `bundle.linux.deb.depends` 鍙槸澹版槑杩愯鏃朵緷璧栬 apt 鑷姩鎷夛紝涓嶅奖鍝?build-time 渚濊禆鈥斺€攂uild 渚濊禆浠嶉渶鎵嬪姩 `apt install *-dev`銆?4) musl 娌℃湁 `__stderr_location` 杩欎釜 glibc 涓撳睘绗﹀彿锛岃法鍙戣鐗堢殑 FFI 瑕侀伩鍏嶄緷璧?glibc 鍐呴儴绗﹀彿銆?|
| 鎴戝仛浜嗕粈涔堬紵 | `tauri.conf.json`锛歵argets 鍔?`deb` + 鏂板 `bundle.linux.deb.depends`銆俙src-tauri/src/main.rs`锛歚setup_file_logging` 鎷嗕笁涓?cfg 鍒嗘敮锛坵indows release 瀹炲師瀹炵幇 / unix release 绌?stub / debug 绌?stub锛夈€俛pt + rustup + npm install 瑁呭ソ鐜銆倀sc/cargo check/tauri:build 鍏?PASS锛宒eb 浜х墿 9.7 MB 鐢熸垚骞堕獙璇併€俻rogress/staging 鏂囨。鍚屾銆?*鐗堟湰淇濇寔 1.11.2锛屾湭杩涘彂甯冩祦姘寸嚎锛屾湭鎺?git锛屾湭鍙?Gitee銆?* |

### 闃舵 55锛氫慨澶?deb 瀹夎澶辫触 鈥斺€?webkit 鍖呭悕 ABI 鐗堟湰閿欙紙libwebkit2gtk-4.1-1 鈫?-0锛夛紙2026-07-18锛?
- **鐜拌薄锛?* 闃舵 54 鐨?deb 鍦?`dpkg -i` 鏃舵姤锛歚my-shell 渚濊禆浜?libwebkit2gtk-4.1-1锛涚劧鑰岋細鏈畨瑁呰蒋浠跺寘 libwebkit2gtk-4.1-1`锛屽寘杩涘叆 half-installed 鐘舵€併€?
- **鏍瑰洜锛?* 闃舵 54 鍦?`tauri.conf.json` 鐨?`bundle.linux.deb.depends` 閲屾妸 webkit 鍖呭悕鍐欐垚浜?`libwebkit2gtk-4.1-1`锛?Debian 鍛藉悕鎯緥閲?`-1` 鏄?SONAME 涓荤増鏈級銆備絾 webkit2gtk 鍦?2.44 涔嬪悗鍋氫簡涓€娆?ABI 閲嶅懡鍚嶏細**鏂扮増 SONAME 鏄?`-0`**锛堝嵆 `libwebkit2gtk-4.1.so.0`锛夛紝Ubuntu 24.04+/Debian 13+/Fedora 41+ 鍏ㄩ儴鍙彁渚?`libwebkit2gtk-4.1-0`锛宍-1` 杩欎釜鍖呭悕鍦ㄥ綋鍓嶅彂琛岀増**瀹屽叏涓嶅瓨鍦?*銆俙apt-cache policy libwebkit2gtk-4.1-1` 鍊欓€夌増鏈负绌洪獙璇佷簡杩欑偣锛沗apt-cache policy libwebkit2gtk-4.1-0` 鍊欓€?`2.52.3-0ubuntu0.26.04.2`銆?
- **鍙︿竴鍧戯細`dpkg -i` 涓嶄細鑷姩瑙ｅ喅渚濊禆**銆傚氨绠楀寘鍚嶅锛岀洿鎺?`dpkg -i xxx.deb` 閬囧埌浠讳綍缂哄け搴撻兘浼氶厤缃け璐ャ€傛纭仛娉曟槸 `sudo apt install ./xxx.deb`鈥斺€攁pt 浼氳鍙?deb 澹版槑鐨勪緷璧栧苟鑷姩浠庝粨搴撴媺鍙栥€傞樁娈?54 README 閲岀粰鐨?`dpkg -i` + `apt --fix-broken install` 鍏滃簳鏂规鑳界敤浣嗕笉浼橀泤锛屾湰娆℃敼涓烘帹鑽?`apt install`銆?
- **淇锛?* `tauri.conf.json` 鎶?`libwebkit2gtk-4.1-1` 鏀逛负 `libwebkit2gtk-4.1-0`锛屽叾浣欎笁涓緷璧栵紙libgtk-3-0 / libsecret-1-0 / libayatana-appindicator3-1锛夊寘鍚嶉兘瀵广€佹棤闇€鏀广€?
- **椤哄甫鎾ゅ洖锛?* 涓€斿皾璇曞姞 `linux.appitem.categories` 閰嶇疆妗岄潰鍏ュ彛鍒嗙被锛屼絾 Tauri v2 schema 娌¤繖涓瓧娈碉紙瀹樻柟 Debian 鏂囨。鏄庣‘锛歞eb 閰嶇疆鍙湁 `files` 鍜?`depends`锛屼笉鏆撮湶 .desktop 鐨?Categories 璁剧疆锛涜鑷畾涔?Categories 蹇呴』鐢?`desktopTemplate`锛屽紩鍏ラ澶栨枃浠剁淮鎶わ級銆傛湰娆℃挙鍥烇紝Categories 鐣欑┖鈥斺€斾笉褰卞搷瀹夎銆佷笉褰卞搷鍥炬爣锛屽彧鏄笉鍑虹幇鍦ㄥ簲鐢ㄨ彍鍗曞垎绫讳笅锛圙NOME 鎼滅储浠嶅彲鎵惧埌锛夈€?
- **楠岃瘉锛?*
  - 閲嶆柊鎵撳寘锛?2 绉掞紝浜岃繘鍒舵病鏀瑰彧閲?bundle锛夛細deb `Depends:` 琛岀幇涓?`libwebkit2gtk-4.1-0, libgtk-3-0, libsecret-1-0, libayatana-appindicator3-1`锛圱auri 鑷姩妫€娴嬪張杩藉姞浜嗛噸澶嶇殑 webkit/gtk锛屾棤瀹筹級銆?
  - `sudo apt install ./MyShell_1.11.2_amd64.deb` 鎴愬姛瀹夎閰嶇疆锛宍dpkg -l my-shell` 鏄剧ず `ii`锛堝凡瀹夎骞堕厤缃級銆?
  - `/usr/bin/myshell` 鍙墽琛岋紝鍚姩鏃ュ織姝ｅ父锛歚[backup] 宸插浠介厤缃枃浠禶銆乣[startup] database initialized`銆乣schema migration ok`锛圙UI 鍥犳祴璇曠幆澧冩棤 display 琚?timeout 鏉€锛屼絾 Rust 鍚庣閫昏緫鍏ㄨ窇閫氾級銆?
  - 妗岄潰鍏ュ彛 `/usr/share/applications/MyShell.desktop` 涓?hicolor 鍥炬爣闆嗭紙32/128/256/512锛夋纭儴缃层€?
  - 娴嬭瘯鍚?`sudo apt remove my-shell` 娓呯悊鏈哄櫒銆?
- **缁撹锛?* Linux deb 鍏ㄩ摼璺墦閫氾紝apt 涓€琛岃濂姐€佽繍琛屾椂渚濊禆鑷姩瑙ｅ喅銆佹闈㈠叆鍙ｄ笌鍥炬爣榻愬叏銆備笅娆″彂甯冿紙`鎵撳寘`锛夊嵆鍙殢 v1.11.3 鎴?v1.12.0 姝ｅ紡涓婄嚎 Linux 浜х墿銆?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 55锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 55 complete 鈥斺€?webkit 鍖呭悕 ABI 鐗堟湰閿欏凡淇紝`apt install ./MyShell_*.deb` 瀹炴祴瀹夎鎴愬姛锛屼簩杩涘埗鑳藉惎鍔ㄣ€?|
| 鎴戣鍘诲摢閲岋紵 | 鐢ㄦ埛鍦ㄥ彟涓€鍙?Ubuntu/Debian 鏈哄櫒瀹炴祴 GUI 鑳芥甯告樉绀恒€丼SH/SFTP/鏈湴缁堢鑳界敤锛涚‘璁ゆ棤璇殢涓嬫 `鎵撳寘` 姝ｅ紡涓婄嚎銆?|
| 鐩爣鏄粈涔堬紵 | `sudo apt install ./MyShell_*_amd64.deb` 涓€琛岃濂斤紝apt 鑷姩鎷変緷璧栵紝搴旂敤鍑虹幇鍦ㄥ簲鐢ㄨ彍鍗曘€佹闈㈠浘鏍囬綈鍏ㄣ€?|
| 鎴戝鍒颁簡浠€涔堬紵 | (1) webkit2gtk 4.1 鍦?2.44 鍚?SONAME 浠?`-1` 鏀规垚 `-0`锛堝皯瑙佺殑 ABI 鐗堟湰鍊掗€€鍛藉悕锛夛紝Ubuntu 24.04+/Debian 13+ 鍏ㄩ儴鐢?`-0`銆?2) `dpkg -i` 涓嶈В鍐充緷璧栥€乣apt install ./xxx.deb` 鎵嶆槸姝ｈВ鈥斺€斿啓鏂囨。鏃剁粰鐨勫懡浠よ鎯虫竻妤氬摢绉嶅満鏅€?3) Tauri v2 鐨?deb 閰嶇疆 schema 闈炲父鍏嬪埗锛堝彧鏈?files + depends锛夛紝.desktop 鐨?Categories 瑕佽嚜瀹氫箟蹇呴』 desktopTemplate锛屾潈琛″悗鐣欑┖鏄洿濂界殑榛樿銆?4) 闃舵 54 娌″疄娴?apt install 灏辨姤"deb 鐢熸垚楠岃瘉閫氳繃"鏄箰瑙備簡鈥斺€旂敓鎴愪笉绛変簬鑳借锛屼笅娆＄被浼煎伐浣滆璺戝埌瀹為檯瀹夎閭ｄ竴姝ャ€?|
| 鎴戝仛浜嗕粈涔堬紵 | `tauri.conf.json`锛歚libwebkit2gtk-4.1-1` 鈫?`-0`锛屾挙鍥炶鍔犵殑 `linux.appitem.categories`銆傞噸鏂版墦鍖?22 绉掋€俙apt install ./deb` 瀹炴祴鎴愬姛锛宍dpkg -l` 鏄剧ず `ii`锛屼簩杩涘埗鍚姩鏃ュ織姝ｅ父锛屾闈㈠叆鍙ｄ笌鍥炬爣榻愬叏锛屾祴璇曞悗 remove 娓呯悊銆俿taging / README / progress 鏂囨。鍚屾鈥斺€擱EADME 鎶?`dpkg -i` 鏀规垚 `apt install ./`銆?*鐗堟湰淇濇寔 1.11.2锛屾湭杩涘彂甯冩祦姘寸嚎锛屾湭鎺?git锛屾湭鍙?Gitee銆?* |

### 闃舵 56锛氬簲鐢ㄥ唴鏇存柊鏀寔骞冲彴鍒嗗彂 + Linux 娴忚鍣ㄨ烦杞ā寮忥紙2026-07-18锛?
- **闇€姹傦細** 闃舵 54/55 璁?Linux 鑳戒骇鍑?deb锛屼絾鐢ㄦ埛闂?褰撳墠鏇存柊鏄惁浼氬尯鍒?Windows 鐗堟湰鍜?Linux 鐗堟湰"銆傝皟鏌ュ悗鍙戠幇锛?*褰撳墠搴旂敤鍐呮洿鏂伴摼璺畬鍏ㄦ槸 Windows 涓撳睘**鈥斺€?
  - `check_for_updates` 鏃犺剳鍙?Gitee release 鐨?`assets[0]` 浣滀负 `download_url`锛屼笉鍖哄垎骞冲彴锛堝鏋滃悓涓€娆?release 鍚屾椂鎸備簡 exe+deb锛屼袱涓钩鍙扮敤鎴烽兘琚寚鍚戝悓涓€涓枃浠讹級
  - `download_update` 鎶婃枃浠朵繚瀛樹负纭紪鐮佺殑 `myshell-update-setup.exe`
  - `install_update` 鍦ㄦ墍鏈夊钩鍙颁笂 `if !lower.ends_with(".exe") { return Err("浠呮敮鎸?.exe 瀹夎鍖?) }`锛屾牴鏈嫆缁濋潪 exe 鏂囦欢
  - 鐢ㄦ埛瑕佺殑鏄?鍖呴噷鏀惧钩鍙版爣璇嗚 Gitee 鑳藉垽瀹?锛岃繖鍏跺疄**涓嶉渶瑕佸湪鍖呴噷鏀句换浣曟爣璇?*鈥斺€旀枃浠跺悕鍚庣紑鏈韩灏辨槸鏍囪瘑锛坄.exe` vs `.deb`锛夛紝浠ｇ爜鍙渶瑕佹寜鍚庣紑杩囨护
- **鏂规閫夊瀷锛?* Linux 鎬庝箞"瑁?涓嬭浇鍒扮殑 deb 鏄鏉傚害鐨勫垎姘村箔銆傚洓涓柟妗?A/B/C/D 澶嶆潅搴﹂€掑锛欰=璺虫祻瑙堝櫒璁╃敤鎴锋墜鍔ㄨ锛堟渶绠€锛屽嚑琛屼唬鐮侊級/ B=涓嬭浇 deb + 寮规彁绀?/ C=pkexec apt install锛堣澶勭悊鎻愭潈+apt杈撳嚭锛? D=tauri-plugin-updater + AppImage锛堥噸鏋勫垎鍙戯級銆?*鐢ㄦ埛閫夋柟妗?A**鈥斺€旀潈琛″悗杩欐槸鏈€绋崇殑锛歀inux 鑷姩瀹夎 deb 娑夊強 pkexec 鎻愭潈寮圭獥銆乤pt 鍛戒护杈撳嚭澶勭悊銆佹浛鎹㈡鍦ㄨ繍琛岀殑 `/usr/bin/myshell` 绛変竴鍫嗚竟鐣屾潯浠讹紝鐣欑粰鍚庣画鏂规鍋氥€?
- **璁捐娲炲療锛堝叧閿渷鍔涚偣锛夛細** 鐜版湁 UI 閲?*宸茬粡**鏈?娴忚鍣ㄤ笅杞?鎸夐挳鈥斺€擿UpdateNotification.tsx` 鐨?failed phase 鍜?`AboutDialog.tsx` 鐨?idle/failed phase 閮芥湁锛岄€氳繃 `openExternalUrl` 鎵撳紑 URL锛涜€?`open_external_url` 鍦?Linux 涓?*宸茬粡鑳藉伐浣?*锛堣蛋 Tauri shell plugin锛岃嚜鍔?xdg-open锛夈€傛墍浠ユ渶绠€鍋氭硶鏄 Linux **瀹屽叏璺宠繃涓嬭浇/瀹夎閾捐矾**锛岀洿鎺ュ鐢ㄧ幇鏈?鎵撳紑娴忚鍣?璺緞锛屽彧闇€瑕佽鍓嶇鐭ラ亾"杩欐槸 browser 妯″紡"銆?
- **浠ｇ爜鏀瑰姩锛?*
  - **`src-tauri/src/main.rs`**锛?
    - `UpdateInfo` struct 鏈熬鍔犲瓧娈?`update_strategy: String`锛坄"auto"` = Windows 璧板唴缃笅杞藉畨瑁?/ `"browser"` = Linux/macOS 璺虫祻瑙堝櫒 / 绌轰覆 = error path锛?
    - `update_info_error` helper 缁欒瀛楁濉┖涓?
    - `check_for_updates` 閲嶅啓 asset 閫夋嫨锛氭寜骞冲彴鎸戝悗缂€锛圵indows `.exe` / Linux `.deb` / 鍏朵粬骞冲彴涓嶆寫锛夛紝鎵句笉鍒板尮閰?asset 鏃堕€€鍥?`release_url`銆傚啀鍔?`update_strategy` 璧嬪€硷紙Windows=auto / 鍏朵粬=browser锛夈€?
  - **`src/api.ts`**锛歚UpdateInfo` interface 闀滃儚 `update_strategy: string`
  - **`src/components/UpdateNotification.tsx`**锛氬姞 `const isBrowserMode = updateInfo.update_strategy === "browser"`锛沺rompt phase 鐨勫壇鏂囨鍦?browser 妯″紡涓嬫敼涓?褰撳墠绯荤粺鏆備笉鏀寔搴旂敤鍐呰嚜鍔ㄦ洿鏂帮紝璇峰墠寰€涓嬭浇椤垫墜鍔ㄤ笅杞藉畨瑁?锛?鏇存柊"鎸夐挳鍦?browser 妯″紡涓嬫浛鎹负"鎵撳紑涓嬭浇椤?锛堢偣鍑?`openExternalUrl(downloadUrl)`锛夈€備笉杩涘叆 downloading/ready phase銆?
  - **`src/components/AboutDialog.tsx`**锛氬悓鏍?`isBrowserMode` 鍒ゆ柇锛沬dle phase 鍦?browser 妯″紡涓嬪彧鏄剧ず"鎵撳紑涓嬭浇椤?鎸夐挳锛堟浛浠?鏇存柊"+"缃戦〉涓嬭浇"缁勫悎锛夛紝鍓枃妗堟敼涓?Linux 璇存槑銆?
  - **`download_update` / `install_update` 瀹屽叏涓嶅姩**鈥斺€擫inux 璺緞鏍规湰涓嶄細璋冪敤瀹冧滑锛屾敼浜嗗弽鑰屽鍔犻闄╅潰銆?
- **Windows 琛屼负闆跺彉鍖栵細** Windows 涓?`cfg!(target_os = "windows")` 璁?`update_strategy = "auto"`銆乤sset 鍚庣紑 = `.exe`锛屼袱涓粍浠剁殑 `isBrowserMode` 閮戒负 false锛屽畬鍏ㄨ蛋鍘?prompt鈫抎ownloading鈫抮eady鈫抜nstall 娴佺▼銆?
- **楠岃瘉锛?*
  - `npx tsc --noEmit` PASS
  - `cargo check --release` PASS锛堝彧鍓╂棤鍏崇殑 `is_openai_protocol` dead_code warning锛?
  - `npm run tauri:build` exit 0锛屼骇鍑烘柊鐗?`MyShell_1.11.2_amd64.deb`锛堝惈鏈樁娈典唬鐮侊級
  - Windows 缂栬瘧涓嶅彈褰卞搷锛坈fg 鍒嗘敮淇濊瘉锛?
- **閬楃暀锛?* Linux deb 鐢ㄦ埛妫€鏌ュ埌鏂扮増鏈椂鍙兘璺虫祻瑙堝櫒鎵嬪姩涓嬭浇瀹夎锛屼笉鏄湡姝ｇ殑"搴旂敤鍐呰嚜鍔ㄦ洿鏂?銆傚鏋滃皢鏉ヨ鍋氭柟妗?C锛坧kexec apt 鑷姩瀹夎锛夛紝鏀瑰姩鐐瑰凡缁忔竻鏅帮細`install_update` 鍔?`#[cfg(target_os = "linux")]` 鍒嗘敮璋?pkexec锛宍download_update` Linux 鍒嗘敮鏀规枃浠跺悕鍚庣紑涓?`.deb`銆傛湰娆′笉鍋氥€?

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 56锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 56 complete 鈥斺€?搴旂敤鍐呮洿鏂版敮鎸佸钩鍙板垎鍙戯紝Linux 璧版祻瑙堝櫒璺宠浆妯″紡锛宼sc/cargo check/tauri:build 鍏?PASS锛屾柊鐗?deb 宸茬敓鎴愩€?|
| 鎴戣鍘诲摢閲岋紵 | git commit + push origin main锛堜笅涓€姝ワ級锛涚敤鎴峰湪 Linux 瀹炴祴"妫€鏌ユ洿鏂?寮圭獥鏄剧ず"鎵撳紑涓嬭浇椤?鎸夐挳涓旇兘璺虫祻瑙堝櫒銆?|

### 闃舵 57锛欳LI + MCP Server 鈥斺€?璁?AI agent 浣跨敤 MyShell锛?026-07-21锛?
- **闇€姹傦細** 鐢ㄦ埛甯屾湜 AI agent锛圕laude Code銆丆ursor銆乑Code 绛夛級鑳介€氳繃鍛戒护琛屽拰 MCP 鍗忚浣跨敤 MyShell 宸蹭繚瀛樼殑 SSH/SFTP 杩炴帴锛屾墽琛岃繙绋嬪懡浠ゃ€佺鐞嗚繙绋嬫枃浠躲€?
- **鏋舵瀯閲嶆瀯锛圥hase 1锛夛細** 鎻愬彇 `myshell_core` 搴擄紙`src-tauri/src/lib.rs`锛夛紝灏?14 涓ā鍧椾粠 main.rs 鐨勭鏈?`mod` 鏀逛负搴撶殑 `pub mod`銆傛牳蹇冭В鑰︼細
  - `State<'_, AppState>` 鈫?`&AppState`锛坰sh/sftp/local/ai 鍥涗釜妯″潡锛寏30 澶勭鍚嶏級
  - `WebviewWindow` 鈫?`Arc<dyn EventSink>`锛堟柊澧?`EventSink` trait + `EventSinkExt` 鎵╁睍 trait锛?
  - `tauri::async_runtime::spawn` 鈫?`tokio::spawn`
  - db/crypto/vault/secrets/proxy/ftp/backup/redact/elevation 闆舵敼鍔紙鏈氨 Tauri-free锛?
  - main.rs 鐦﹁韩涓虹函 Tauri 鍛戒护鍖呰灞?+ `WindowSink` 閫傞厤鍣?
- **CLI 浜岃繘鍒讹紙Phase 2锛夛細** `src-tauri/src/bin/myshell-cli.rs`锛宑lap derive API锛?
  - `myshell-cli list [--json]` 鈥?鍒楀嚭杩炴帴
  - `myshell-cli exec <杩炴帴鍚? "鍛戒护" [--json] [--timeout N]` 鈥?杩滅▼鍛戒护鎵ц锛圓I 鏍稿績鍦烘櫙锛?
  - `myshell-cli sftp ls/get/put/mkdir/rm/rename` 鈥?SFTP 鏂囦欢鎿嶄綔
  - `myshell-cli ssh <杩炴帴鍚?` 鈥?浜や簰寮忕粓绔?
  - `myshell-cli test <杩炴帴鍚?` 鈥?杩炴帴娴嬭瘯
  - Vault 瑙ｉ攣锛歚MYSHELL_PASSPHRASE` 鐜鍙橀噺 > `--passphrase` 鍙傛暟 > 浜や簰鎻愮ず锛坮password锛?
  - `--json` 鍏ㄥ眬閫夐」杈撳嚭鏈哄櫒鍙 JSON
- **MCP Server锛圥hase 3锛夛細** `src-tauri/src/bin/myshell-mcp.rs`锛屾墜鍐?MCP 鍗忚锛圝SON-RPC 2.0 over stdio锛孋ontent-Length 甯э級锛?
  - 9 涓?tools锛歚list_connections`銆乣ssh_exec`銆乣sftp_list`銆乣sftp_download`銆乣sftp_upload`銆乣sftp_mkdir`銆乣sftp_remove`銆乣sftp_rename`銆乣test_connection`
  - 鍚姩鏃惰 `MYSHELL_PASSPHRASE` 鐜鍙橀噺瑙ｉ攣 vault
  - 鍏煎 Claude Desktop / Cursor / ZCode 绛?MCP 瀹㈡埛绔?
- **Cargo.toml 鍙樻洿锛?* 鏂板 `[lib] name = "myshell_core"` + `[[bin]] myshell-cli` + `[[bin]] myshell-mcp` + clap/rpassword 渚濊禆
- **楠岃瘉锛?* `cargo check` 鍥涗釜鐩爣锛坙ib + 3 bin锛夊叏閮?PASS锛沗npx tsc --noEmit` PASS锛堝墠绔棤褰卞搷锛?
- **AI 瀹㈡埛绔厤缃ず渚嬶細**
  ```json
  { "mcpServers": { "myshell": {
      "command": "myshell-mcp",
      "env": { "MYSHELL_PASSPHRASE": "your-master-password" }
  }}}
  ```

### 闃舵 58锛歁CP 绠＄悊璁剧疆椤?+ keyring 瀹夊叏鍑瘉锛?026-07-21锛?
- **闇€姹傦細** 鍦?GUI 璁剧疆闈㈡澘涓坊鍔?MCP 鏀寔绠＄悊椤甸潰锛岃鐢ㄦ埛鍙互鏂逛究鍦板惎鐢?绂佺敤 MCP銆佺鐞嗗瘑鐮侊紙Windows 鍑瘉绠＄悊鍣?DPAPI 鍔犲瘑锛夈€佽嚜鍔ㄦ娴嬪拰閰嶇疆 AI 宸ュ叿銆?
- **鍚庣鏀瑰姩锛?*
  - `src-tauri/src/mcp_tools.rs`锛堟柊妯″潡锛夛細AI 宸ュ叿瀹夎妫€娴嬶紙Claude/Opencode/Zcode锛夈€侀厤缃鍐欙紙鍘婚噸妫€娴嬶級銆佷簩杩涘埗璺緞瑙ｆ瀽
  - `src-tauri/src/secrets.rs`锛氭柊澧?`set_mcp_passphrase` / `get_mcp_passphrase` / `delete_mcp_passphrase`锛坘eyring 鏈嶅姟 `myshell-mcp`锛?
  - `src-tauri/src/main.rs`锛? 涓?MCP Tauri commands锛坉etect_tools, write_config, remove_config, save_passphrase 绛夛級
  - `src-tauri/src/bin/myshell-cli.rs`锛歚vault save-passphrase` 鍛戒护锛堥獙璇佸瘑鐮佸悗鍐欏叆 keyring锛?
- **鍓嶇鏀瑰姩锛?*
  - `src/api.ts`锛? 涓?MCP API 灏佽鍑芥暟 + `AiToolInfo` 鎺ュ彛
  - `src/components/SettingsPanel.tsx`锛氭柊澧?MCP鏀寔"渚ф爮鍒嗙被锛屽寘鍚細鍚敤/绂佺敤寮€鍏炽€乿ault 瀵嗙爜鍚屾鍒?keyring銆佷慨鏀?鍒犻櫎 keyring 瀵嗙爜銆丄I 宸ュ叿鑷姩妫€娴嬩笌涓€閿厤缃€佸瘑鐮佷慨鏀规椂鑷姩鍚屾 keyring
- **瀹夊叏鏀硅繘锛?* 寮冪敤 `MYSHELL_PASSPHRASE` 鐜鍙橀噺鏄庢枃鏂规锛屾敼鐢?Windows 鍑瘉绠＄悊鍣紙DPAPI 鍔犲瘑锛夊瓨鍌?vault 涓诲瘑鐮?
- **楠岃瘉锛?* `npx tsc --noEmit` PASS锛宍cargo check` 鍏ㄩ儴 PASS

## 浜旈棶閲嶅惎妫€鏌ワ紙闃舵 58锛?
| 闂 | 绛旀 |
|------|------|
| 鎴戝湪鍝噷锛?| 闃舵 57 complete 鈥斺€?CLI + MCP Server 瀹炵幇瀹屾瘯锛宑argo check 鍥涚洰鏍囧叏 PASS锛宼sc PASS銆?|
| 鎴戣鍘诲摢閲岋紵 | git commit + push锛涚敤鎴峰疄娴?`myshell-cli list`銆乣myshell-cli exec`銆丮CP 瀹㈡埛绔繛鎺ャ€?|
| 浠€涔堝彲鑳藉鑷村亸绂伙紵 | russh-sftp API 鍦ㄦ枃浠朵紶杈撴椂鍙兘鏈夎竟鐣岄棶棰橈紙澶ф枃浠躲€佺鍙烽摼鎺ワ級锛汳CP 鍗忚甯цВ鏋愬湪闈炴爣瀹㈡埛绔笂鍙兘闇€瑕佽皟鏁淬€?|
| 涓嬩竴姝ユ渶灏忓彲楠岃瘉鍔ㄤ綔锛?| `cargo build --release` 浜у嚭涓変釜浜岃繘鍒讹紝鎵嬪姩杩愯 `myshell-cli vault status` 楠岃瘉鍩虹鍔熻兘銆?|
| 閬楃暀鍊哄姟锛?| CLI 浜や簰寮?SSH 妯″紡鍦?Windows 涓婂彲鑳介渶瑕?raw mode 缁堢澶勭悊锛堝綋鍓嶆槸琛屾ā寮忥級锛汳CP Server 鏃犺繛鎺ユ睜锛堟瘡娆?tool call 鏂板缓 SSH 杩炴帴锛夈€?|
| 鐩爣鏄粈涔堬紵 | Linux deb 鐢ㄦ埛涔熻兘鎰熺煡鍒版柊鐗堟湰骞舵嬁鍒板搴斿钩鍙扮殑瀹夎鍖咃紝涓嶄細琚敊璇湴鎸囧悜 Windows exe銆?|
| 鎴戝鍒颁簡浠€涔堬紵 | (1) "鍖呴噷鏀惧钩鍙版爣璇?鏄啑浣欑殑鈥斺€旀枃浠跺悕鍚庣紑锛?exe/.deb锛夋湰韬氨鏄爣璇嗭紝浠ｇ爜鎸夊悗缂€杩囨护鍗冲彲銆?2) 澶嶇敤宸叉湁 UI 姣旀柊鍐?UI 鐪?90% 宸ヤ綔閲忥細鐜版湁 failed-phase 鐨?娴忚鍣ㄤ笅杞?鎸夐挳 + `open_external_url` 鍦?Linux 涓婂凡缁忚兘鐢紙Tauri shell plugin 鍐呯疆 xdg-open锛夛紝鍙渶鍔犱釜 `update_strategy` 瀛楁璁╁墠绔煡閬撹蛋鍝潯璺€?3) Linux 鑷姩瀹夎 deb 鐨勫鏉傚害杩滆秴"鎵撳寘"鈥斺€攑kexec/apt/鏇挎崲杩愯涓簩杩涘埗閮芥槸杈圭晫鏉′欢锛屾柟妗?A锛堣烦娴忚鍣級鏄姟瀹為€夋嫨銆?4) `cfg!(target_os = ...)` 鍦?Rust 琛ㄨ揪寮忛噷鑳界敤銆乣cfg!(windows)` 鍦ㄥ睘鎬т綅缃敤鈥斺€斿悓涓€姒傚康涓ょ璇硶銆?|
| 鎴戝仛浜嗕粈涔堬紵 | `main.rs`锛歎pdateInfo 鍔?`update_strategy`銆乣check_for_updates` 鎸夊钩鍙扮瓫 asset + 璁剧瓥鐣ャ€乣update_info_error` 琛ュ瓧娈点€俙api.ts`锛氶暅鍍忓瓧娈点€俙UpdateNotification.tsx` + `AboutDialog.tsx`锛歜rowser 妯″紡鍒嗘敮锛堟寜閽?鏂囨锛夈€俙download_update`/`install_update` 涓嶅姩銆傞妫€鍏?PASS锛屾柊鐗?deb 鐢熸垚銆俿taging + progress 鍚屾銆?*鐗堟湰淇濇寔 1.11.2**锛屼笅涓€姝?commit + push origin main锛堜笉鍙?Gitee release锛夈€?|

### 闃舵 59锛歁CP 楂樺嵄鎿嶄綔浜哄伐纭鏈哄埗锛?026-07-21锛?
- **闇€姹傦細** AI agent 閫氳繃 MCP 鎵ц楂樺嵄鎿嶄綔锛堣繙绋嬪懡浠ゆ墽琛屻€佸垹闄ゆ枃浠躲€侀噸鍛藉悕銆佷笂浼犺鐩栵級鏃讹紝蹇呴』寮瑰嚭 OS 绾х‘璁ゅ璇濇锛岀敤鎴风偣鍑?纭"鍚庢墠鎵ц锛岀偣鍑?鍙栨秷"鍒欐嫆缁濄€備笉鍏佽 AI 璺宠繃纭銆?
- **瀹炵幇锛?*
  - `src-tauri/src/bin/myshell-mcp.rs`锛氭柊澧?`confirm_dangerous_operation()` 鍑芥暟锛岃皟鐢?Windows API `MessageBoxW` 寮瑰嚭妯℃€佽鍛婂璇濇锛圡B_YESNO | MB_ICONWARNING | MB_SYSTEMMODAL锛?
  - 鍦?`ssh_exec`銆乣sftp_remove`銆乣sftp_rename`銆乣sftp_upload` 鍥涗釜楂樺嵄 tool 澶勭悊鍓嶆彃鍏ョ‘璁ゆ鏌?
  - 纭閫氳繃鎵嶇户缁墽琛岋紝鎷掔粷鍒欒繑鍥?`isError: true` 鐨?JSON-RPC 鍝嶅簲
  - 闈?Windows 鐜锛氭嫆缁濋珮鍗辨搷浣滃苟鎵撳嵃璀﹀憡
- **璁捐鍐崇瓥锛?* MCP server 鏄敱 AI 宸ュ叿鍚姩鐨勭嫭绔嬭繘绋嬶紝鏃犳硶鐩存帴涓?GUI 閫氫俊銆傞噰鐢?OS 鍘熺敓瀵硅瘽妗嗘柟妗堬紝涓嶄緷璧?GUI 杩涚▼锛岀嫭绔嬪彲闈犮€?
- **楠岃瘉锛?* `cargo check --bin myshell-mcp` PASS



### 阶段 60：修复 MCP server 连接超时（ZCode 30000ms）（2026-07-21）
- **症状：** ZCode（Desktop）启动 myshell-mcp.exe 后，30 秒后报 "MCP server myshell connection timed out after 30000ms"。手动 node 测试（pipe 注入 initialize）则正常。
- **根因：** MCP server 启动时在主 async 任务里**同步**调用 vault unlock（PBKDF2-HMAC-SHA256 600k 次迭代 ≈ 1 秒），这段时间主线程被占满，`read_message` 无法响应 ZCode 发来的 initialize。ZCode 等不到 initialize 响应就判定超时。
- **修复：**
  - `src-tauri/src/bin/myshell-mcp.rs`：提取 `try_unlock_dek(passphrase) -> Result<[u8;32], String>`，vault 解锁逻辑搬到 `std::thread::spawn` 的后台线程，通过 `Arc::clone(&app.dek)` 把 DEK 写回。主 async 任务立即进入 JSON-RPC 循环。
  - 加详细诊断日志：recv/sent 方法名 + id 写到 `%APPDATA%\\myshell\\logs\\mcp.log`（stdout 严格保留给 JSON-RPC，不能写任何调试输出）。
  - 删除 `unlock()` 死代码（被 `try_unlock_dek` 取代）。
- **验证：** 手动 node 模拟 ZCode 完整握手，initialize 3ms 响应，tools/list 3ms 响应，9 个工具全部列出。日志确认 `recv: initialize` → `sent: initialize response` → `recv: tools/list` → `sent: tools/list response` 完整链路。

## 五问重启检查（阶段 60）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 60 complete —— MCP 连接超时已修复，手动模拟 ZCode 握手通过，9 个工具全部暴露。 |
| 我要去哪里？ | 用户重启 ZCode 确认实际生效，然后打包完整 NSIS 安装包（含修复后的 mcp 二进制）。 |
| 什么可能导致偏离？ | (1) ZCode 可能有自定义的超时/握手要求，比官方 MCP spec 更严；(2) 后台线程解锁完成前调用加密连接 tool 会失败（预期行为，会返回明确错误）。 |
| 下一步最小可验证动作？ | 用户在 ZCode 设置 → MCP 看到 myshell 变成"已连接 + 9 个工具"；或者调用 `list_connections` tool 看到保存的连接列表。 |
| 目标是什么？ | ZCode（及其他 AI agent）能稳定连接 myshell-mcp，调用 SSH/SFTP tool，高危操作弹 OS 对话框。 |


### 阶段 61：MCP 工具描述 + server-level instructions（让 AI 知道何时调用）（2026-07-21）
- **需求：** AI agent 是否会在正确场景调用 myshell MCP，取决于工具的 description 写得够不够明确。当前 description 太简短（如"列出远程目录"），AI 无法区分该用 myshell 还是自己的 shell 工具。
- **改动：**
  - **`tool_definitions()` 9 个工具 description 全部重写**：每个工具都包含 (1) WHEN TO USE 触发场景，(2) WHEN NOT TO USE 反例（关键！让 AI 知道何时不要用），(3) OUTPUT 格式，(4) SIDE EFFECTS / 危险提示（⚠️ HUMAN CONFIRMATION REQUIRED）。英文书写以兼容所有 AI 客户端。
  - **新增 `SERVER_INSTRUCTIONS` 常量**：MCP 2025-06-18 规范新字段，放在 initialize 响应里。客户端会把它 prepend 到系统提示，告诉 AI 整个 myshell MCP 的定位（"this user's pre-saved server connections"）、触发关键词（"on prod-db" / "check web1"）、工作流（先 list_connections 取名字再调用）、安全模型（4 个高危工具会弹 OS 对话框）。
  - `protocolVersion` 保持 2024-11-05（最大兼容性，spec 同主版本双向兼容；新字段 instructions 会被老客户端忽略）。
- **验证：** `cargo build --release --bin myshell-mcp` PASS，`opencode mcp list` 3 个 MCP server（chrome-devtools / codegraph / myshell）全部 connected。

## 五问重启检查（阶段 61）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 61 complete —— 工具描述和 server instructions 都已加上，opencode 验证连上。 |
| 我要去哪里？ | 用户重启 ZCode 实测 AI 是否正确识别并调用 myshell 工具。 |
| 什么可能导致偏离？ | (1) 部分老 MCP 客户端不支持 instructions 字段，但 description 仍然生效；(2) AI 可能仍然倾向于用自己的 shell 工具（习惯了），需要用户明确说"用 myshell"几次才会内化。 |
| 下一步最小可验证动作？ | 用户在 ZCode 里问 "帮我看下 xxx 服务器上的 nginx 配置"，看 AI 是否主动调 list_connections → sftp_download。 |
| 目标是什么？ | AI 在用户提到已保存的服务器时，自动选择 myshell MCP 工具，而不是用本地 shell 或询问连接信息。 |


### 阶段 62：MCP 连接查找支持 host/IP + conn_type 自动消歧（2026-07-21）
- **需求：** 用户问 "ssh 到 135.32.64.30" 时，希望 AI 能直接通过 IP 找到对应连接并执行。但用户实际有两个重名连接（同一 IP 既存 ssh 又存 sftp，name 都叫 IP），原 find_connection 用 `c.name == name` 匹配会命中第一条（sftp 那个），导致认证方式不对、失败。
- **改动：**
  - **`find_connection(query, expected_conn_type)`** 重写：
    - 接受三种 query 形式：name（如 `prod-db`）/ group-path（如 `/production/prod-db`）/ host 或 IP（如 `135.32.64.30`）
    - 三轮匹配：name 精确 → group-path 精确 → host 精确，每轮都按 `expected_conn_type` 过滤
    - 单命中直接返回；多命中返回错误并列出候选项让 AI 转告用户
    - 0 命中 + 有 type 提示时，额外检查不带 type 是否有命中，给出更友好错误（"找到了但类型是 sftp 不是 ssh"）
  - **`ambiguous_error()`** 新增：格式化歧义错误，列出每个候选项的 type/group/user
  - **8 个工具调用点改造**：ssh_exec → `Some("ssh")`；7 个 sftp_* → `Some("sftp")`；test_connection → `None`（接受任意类型）
  - **工具描述 + SERVER_INSTRUCTIONS 同步更新**：明确告知 AI 可以用 name/group-path/host/IP 三种形式调用；conn_type 自动消歧；歧义时工具会列出候选项
- **验证：** 实测 `ssh_exec {"connection":"135.32.64.30"}` 成功找到 ssh 类型那条连接（ias 用户），走到了 dial 阶段（仅因目标在内网无法连通才超时，查找逻辑本身正确）。`cargo build --release --bin myshell-mcp` PASS。

## 五问重启检查（阶段 62）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 62 complete —— IP/host 查找 + conn_type 消歧全部实现并验证。 |
| 我要去哪里？ | 用户重启 ZCode 实测："ssh 到 XXX.XXX.XXX.XXX" 应能直接找到对应 ssh 连接。 |
| 什么可能导致偏离？ | (1) 同一 host 在同一 type 下存了多条（比如两个 ssh 到同一 IP 不同端口），仍会歧义——但这是用户数据问题，错误信息会引导。(2) AI 可能不主动用 IP，习惯先 list_connections 再传 name——可以接受。 |
| 下一步最小可验证动作？ | 用户问 "ssh 到 135.32.64.30 跑下 uname -a"，看 AI 是否跳过 list_connections 直接调 ssh_exec。 |
| 目标是什么？ | AI 用最自然的方式（名字/别名/IP）调用 myshell，对重名/多类型连接也能自动或经用户确认后选中正确的。 |


### 阶段 63：终端截图功能 + 附件目录 + MCP screenshot_terminal 工具（2026-07-21）
- **需求：** (1) CommandBar 加 📷 截图按钮，紧挨"快捷"按钮；(2) 截图范围仅终端 viewport，不含工具栏/输入命令栏/标签栏；(3) 截图自动保存到附件目录；(4) 附件目录在「设置 → MCP 支持」配置，首次打开提示；(5) MCP 新增 screenshot_terminal 工具，允许 AI 触发。
- **架构决策：**
  - 截图技术：html2canvas（primary）+ 直接复制 xterm 内部 canvas 像素（fallback，应对 webgl renderer 的空白问题）
  - 截图目标：`term.element`（`.xterm` DOM 根）——CommandBar 是兄弟节点不是子节点，天然排除
  - MCP screenshot_terminal：MCP server 是独立进程，无法访问 GUI 的 xterm DOM。采用"诚实告知"方案——工具返回一段中文指引让 AI 转告用户去 GUI 点 📷，并预先把连接信息、附件目录路径（或"未配置"提示）嵌入。避免假装能做到实际做不到的事。
- **改动：**
  - **后端** (`src-tauri/src/main.rs`)：
    - `get_attachment_dir` / `set_attachment_dir`（仿 `gpu_disabled_flag_path`，文件 `<config_dir>/myshell/attachment-dir` 存路径）
    - `save_screenshot(data_url, connection_name)`：base64 解码 PNG → 写入 `截图_<连接名>_<时间戳>.png` → 返回完整路径
    - `show_in_folder(path)`：跨平台打开文件管理器并选中文件（Windows: `explorer.exe /select,`，macOS: `open -R`，Linux: `xdg-open`）
  - **前端 API** (`src/api.ts`)：4 个封装函数
  - **截图工具** (`src/utils/screenshot.ts` 新文件)：`captureTerminalToDataUrl(term)` 返回 PNG dataUrl；html2canvas 主路径，canvas fallback；空白 PNG 检测自动切换 fallback
  - **CommandBar** (`src/components/CommandBar.tsx`)：新增 📷 按钮，相同样式，紧挨"快捷"按钮；Props 加 `onScreenshot`
  - **TerminalPanel** (`src/components/TerminalPanel.tsx`)：
    - Props 加 `connectionName`（用于文件名）
    - `handleScreenshot()`：captureTerminalToDataUrl → saveScreenshot → toast 横幅（"正在截取"→"已保存：路径"+"打开"按钮 / "错误"）
    - 截图状态 toast 4s 自动消失
  - **App.tsx**：`connectionName={tab.name}` 透传
  - **SettingsPanel** (`src/components/SettingsPanel.tsx`)：MCP 节新增"📎 附件目录"子区块，含「选择目录」/「打开目录」按钮；首次未配置显示警告横幅（localStorage 标记"已知晓"避免反复弹）
  - **MCP** (`src-tauri/src/bin/myshell-mcp.rs`)：`screenshot_terminal` 工具定义 + 分发；`secrets_attachment_dir()` 辅助函数读附件目录
- **验证：** `cargo check` PASS，`npx tsc --noEmit` PASS，`cargo build --release --bin myshell-mcp` PASS，`opencode mcp list` myshell connected（10 个工具）。

## 五问重启检查（阶段 63）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 63 complete —— 截图按钮、附件目录、MCP screenshot 工具全部实现并编译通过。 |
| 我要去哪里？ | 用户实测：(1) 设置 → MCP 支持 → 配置附件目录；(2) 打开任意 SSH tab → 点 📷 → 检查生成的 PNG 只含终端不含工具栏；(3) MCP screenshot_terminal 返回合理指引。 |
| 什么可能导致偏离？ | (1) html2canvas 对 webgl renderer 可能仍空白——有 canvas fallback 兜底；(2) 首次截图前必须配置附件目录，否则会报错（已在 toast 中提示）；(3) xterm DOM 结构在新版本可能变化，导致 canvas fallback 路径找不到 .xterm-screen。 |
| 下一步最小可验证动作？ | 用户打开 SSH tab 输出些内容（比如 ls / uname -a），点 📷，确认附件目录有新 PNG 且内容正确。 |
| 目标是什么？ | 一键截取当前终端画面用于分享/留档；AI 能通过 MCP 引导用户完成截图并拿到文件位置。 |


### 阶段 64：v2.0.0 发布（2026-07-21）

**版本：** 1.11.2 → **2.0.0**（用户指定，重大版本升级）

**发布内容：** 累积 v1.11.2 之后的全部改动，按用户确认的 CHANGELOG 节发布（无 🐛 修复 节，只保留 ✨新增 / 🛠️优化 / 🔒安全）。

**完整发布流水线：**
1. ✅ 版本号 bump：Cargo.toml 1.11.2 → 2.0.0，npm run version:sync 同步 package.json/lock
2. ✅ CHANGELOG.md + README.md 更新日志节同步
3. ✅ git diff --stat 完整性检查：所有改动文件都已覆盖 staging 条目
4. ✅ ⚠️ 确认门：用户审阅版本号 + CHANGELOG 全文 → 修改（删 🐛 修复节）→ 再次确认通过
5. ✅ 预检：npx tsc --noEmit + cargo check 全部 PASS
6. ✅ 构建：scripts/build-release.bat → MCP + CLI 二进制先编，再 tauri:build → MyShell_2.0.0_x64-setup.exe
7. ✅ commit + push：5293c88 release: v2.0.0 → origin/main
8. ✅ Gitee 发布：https://gitee.com/argustang/myshell/releases/tag/v2.0.0（release id=755644，exe 上传完成）
9. ✅ 清理：RELEASE_NOTES_STAGING.md 待发布条目清空，baseline 更新为 v2.0.0

**核心交付：**
- **CLI（myshell-cli）**：命令行访问已保存 SSH/SFTP 连接
- **MCP Server（myshell-mcp）**：10 个工具暴露给 AI agent，NDJSON 协议，高危操作人工确认
- **三二进制架构**：提取 myshell_core 共享库
- **终端截图**：CommandBar 📷 按钮，读 xterm buffer 自绘，附件目录管理
- **架构修复**：NDJSON 协议分帧、MCP 连接超时（后台解锁）、Cargo default-run

## 五问重启检查（阶段 64）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | v2.0.0 已发布到 Gitee，commit 已 push，staging 已清空。 |
| 我要去哪里？ | v2.0.x 维护期——等用户反馈，按需打 patch；或开始 v2.1 新特性。 |
| 什么可能导致偏离？ | (1) 用户安装后发现回归 bug → 需要 hotfix patch；(2) MCP 客户端（ZCode/Cursor 等）可能反馈新的兼容性问题。 |
| 下一步最小可验证动作？ | 用户下载 MyShell_2.0.0_x64-setup.exe 覆盖安装，验证：①旧连接/密码能正常加载；②截图按钮工作；③MCP 在 ZCode/opencode 中连接正常。 |
| 目标是什么？ | v2.0.0 稳定可用，AI agent 集成能力交付到用户手中。 |


### 阶段 65：MCP open_in_gui —— AI 驱动 GUI 开新 tab（2026-07-21）

- **需求：** AI agent 通过 MCP 让 MyShell GUI 自动打开某个已保存连接的终端 tab、自动连接、窗口聚焦到前台。用户说"在 MyShell 里打开 prod-db"即可，无需手动找连接双击。
- **架构：localhost TCP IPC 桥**（MCP server 与 GUI 是两个独立进程，此前无任何 IPC 通道）：
  ```
  AI → MCP server → 读 <config_dir>/myshell/gui-ipc-port 拿端口
       → TCP 连 127.0.0.1:port 发 {"action":"open_connection","connection_id":...}
       → GUI IPC listener 线程收到 → window.show()+set_focus()
       → emit("mcp-gui-command") → 前端 App.tsx listen → handleConnect(config)
       → 新 tab 打开 + SSH 连接
  ```
  选 TCP 而非 deep-link/single-instance 插件：零新 crate 依赖（Rust std TcpListener）、不需注册 URI scheme/不需管理员权限、实时、只绑 127.0.0.1 安全。
- **改动：**
  - `src-tauri/src/main.rs`：setup 里 spawn IPC listener 后台线程（bind 127.0.0.1:0 随机端口 → 写 gui-ipc-port 文件 → accept 循环读 NDJSON 命令 → emit 事件 + 聚焦窗口 + 回写 {"ok":true}）；RunEvent::ExitRequested 里删除端口文件。
  - `src/App.tsx`：新增 useEffect 监听 `mcp-gui-command` 事件，按 connection_id 在 connections 里查找并调 `handleConnect(config)`（与侧边栏双击同一代码路径）。
  - `src-tauri/src/bin/myshell-mcp.rs`：新增 `open_in_gui` 工具（第 11 个）+ `read_gui_ipc_port()` / `send_gui_open_command()` 辅助函数。GUI 未运行时返回明确错误并建议改用 ssh_exec。
- **验证：** `cargo check` PASS，`npx tsc --noEmit` PASS。node 端到端测试：MCP 解析连接 135.32.56.63 → TCP 连 GUI:3635 → 返回成功消息（IPC 桥 TCP 往返 + emit 全部成功）。opencode mcp list 确认 11 工具 connected。
- **已知边界：** GUI 在主密码登录门时 connections 未加载，此时 open_in_gui 事件到达但前端查不到连接（需已登录）。进程被强杀（非正常关闭）时端口文件可能残留指向死端口——MCP 连接失败会优雅报错。

## 五问重启检查（阶段 65）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 65 complete —— open_in_gui 工具 + IPC 桥实现并验证（TCP 往返成功，11 工具注册）。 |
| 我要去哪里？ | 用户在已登录的 GUI 里实测：让 AI 调 open_in_gui，确认 GUI 真的开了 tab 并聚焦。 |
| 什么可能导致偏离？ | (1) GUI 在登录门时连接打不开（需先登录）；(2) 进程强杀残留端口文件（已优雅处理）；(3) 前端 listener 依赖 connections，登录前为空。 |
| 下一步最小可验证动作？ | 用户登录 GUI 后，在 AI 对话里说"在 MyShell 打开 xxx 服务器"，看 GUI 是否弹出新 tab 并聚焦。 |
| 目标是什么？ | AI 能一键把用户已保存的连接在 GUI 里可视化打开，打通"AI 后台执行"与"用户前台交互"两种模式。 |


### 阶段 66：open_in_gui 增强 —— tab_type + 聚焦已有 tab（2026-07-22）

- **需求：** 在阶段 65 的 open_in_gui 基础上扩展：(1) 支持开 SFTP 文件浏览 tab（不只是终端）；(2) 同一连接已打开时聚焦已有 tab 而非重复开新 tab。
- **改动：**
  - **MCP 工具**（`myshell-mcp.rs`）：`open_in_gui` 新增 `tab_type` 参数（`auto`/`terminal`/`sftp`，默认 auto）；description 详述三种 tab 类型与聚焦行为；分发逻辑读 tab_type 并传给 GUI；成功消息按 tab 类型动态生成。
  - **IPC 协议**：`send_gui_open_command` 签名加 `tab_type` + `focus_existing` 参数，命令 JSON 携带这两字段；GUI listener（`main.rs`）透传到 `mcp-gui-command` 事件 payload。
  - **前端**（`App.tsx`）：listener 升级——按 `focus_existing`（默认 true）先查已有匹配 tab（按 connectionId + tab 类型过滤），命中则 `setActiveTabId` 切换不新开；未命中则 `handleConnect`，`tab_type=sftp` 时对 SSH 连接覆盖 `conn_type="sftp"` 强制开文件浏览 tab。useEffect 依赖加 `tabs`。
- **验证：** `cargo check` + `npx tsc --noEmit` PASS。聚焦行为实测：同一连接调 open_in_gui 两次，GUI 日志 `connect requested` 仅 1 条（第二次聚焦已有 tab 未新开）——证明聚焦逻辑正确。

## 五问重启检查（阶段 66）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 66 complete —— open_in_gui 增强（tab_type + 聚焦）实现并验证（聚焦实测 2 调用 1 连接）。 |
| 我要去哪里？ | 打包 v2.1.0 发布（用户指示无需确认门）。 |
| 什么可能导致偏离？ | (1) sftp 覆盖对 ftp/local 连接无意义（已忽略覆盖）；(2) 聚焦匹配按 connectionId+类型，同连接多 tab 时聚焦第一个匹配。 |
| 下一步最小可验证动作？ | 打包 v2.1.0，用户安装后让 AI 调 open_in_gui tab_type=sftp 看是否开文件浏览 tab。 |
| 目标是什么？ | AI 能灵活驱动 GUI 开终端/SFTP tab，且智能聚焦避免重复 tab。 |


### 阶段 67：v2.1.0 发布（2026-07-22）

**版本：** 2.0.0 → **2.1.0**（minor，含 ✨ 新增 open_in_gui）

**发布内容：** open_in_gui 工具（阶段 65 + 66 累积）——AI 驱动 GUI 开连接 tab，支持 tab_type（auto/terminal/sftp）+ 智能聚焦已有 tab。

**完整发布流水线：**
1. ✅ 版本 bump：Cargo.toml 2.0.0 → 2.1.0，version:sync 同步 package.json/lock
2. ✅ CHANGELOG.md + README.md 更新日志节同步
3. ✅ 预检：npx tsc --noEmit + cargo check 全 PASS
4. ✅ 构建：scripts/build-release.bat → MyShell_2.1.0_x64-setup.exe（8.99 MB）
5. ✅ commit + push：6376e1a release: v2.1.0 → origin/main
6. ✅ Gitee 发布：https://gitee.com/argustang/myshell/releases/tag/v2.1.0（release id=755892，exe 上传完成）。首次发布因 VPN 改变路由导致 Gitee API 端点（180.76.199.x）连接超时，网络恢复后重试成功。
7. ✅ 清理：RELEASE_NOTES_STAGING.md 待发布条目清空，baseline 更新为 v2.1.0

## 五问重启检查（阶段 67）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | v2.1.0 已发布到 Gitee，commit 已 push，staging 已清空。 |
| 我要去哪里？ | v2.1.x 维护期——等用户反馈 open_in_gui 实际使用体验，按需 patch；或开始 v2.2 新特性。 |
| 什么可能导致偏离？ | (1) Gitee API 在 VPN 下不稳定（发布脚本可考虑加代理支持）；(2) open_in_gui 的 sftp 覆盖、聚焦匹配在多 tab 场景可能有边界。 |
| 下一步最小可验证动作？ | 用户安装 v2.1.0，让 AI 调 open_in_gui（含 tab_type=sftp 和重复调用聚焦）验证日常可用。 |
| 目标是什么？ | AI ↔ GUI 联动能力稳定交付，用户能用自然语言驱动 MyShell 开终端/文件浏览 tab。 |


### 阶段 68：ssh_exec 命令确认规则（黑名单正则 + 白名单豁免，可配置）（2026-07-22）

- **需求：** 原本 ssh_exec 对任何命令都弹人工确认框（ps/ls 也不例外），用户反馈 ps aux 不该算高危。改为：黑名单（危险命令）才确认，白名单（只读命令的误报）豁免，正则匹配，规则可在 GUI 编辑。
- **设计：黑名单为主 + 白名单豁免 + 正则匹配**：
  - 判定顺序：① 命令替换 `$(`/`反引号`/写重定向 `> 文件`/管道到 shell → 始终确认（硬底线，不可配置）；② 黑名单正则命中且白名单正则未命中 → 确认；③ 黑名单未命中 → 放行（confirm_unknown=true 时未知也确认）。
  - 黑名单正则：`(^|[;&|]\s*)rm\b` 等（匹配命令位置，避免 echo rm 误报；约 70 条默认）。
  - 白名单正则：`(^|[;&|]\s*)grep\b` 等（豁免 grep/ps/ls/cat 等只读命令的误报；约 7 条默认）。
  - 白名单只在黑名单命中时检查，且按命令位置匹配（`cat a; rm b` 不会被 cat 豁免，因为 rm 在独立链段）。
- **改动：**
  - `src-tauri/src/command_rules.rs`（新模块，myshell_core 库）：CommandRules 结构体（blacklist/whitelist/confirm_unknown）+ 默认列表 + `command_needs_confirmation()` + `has_write_redirect()`/`has_command_substitution()` 辅助 + **19 个单元测试**（ps 免确认 / rm 确认 / grep rm 豁免 / cat;rm 仍确认 / >/dev/null 放行 / curl|bash 确认 等）。
  - `src-tauri/src/lib.rs`：`pub mod command_rules;`
  - `src-tauri/src/main.rs`：`get_command_rules`/`set_command_rules` 命令（JSON 文件 `mcp-command-rules.json`，仿 attachment_dir 模式）+ 注册。
  - `src/api.ts`：CommandRules 接口 + getCommandRules/setCommandRules。
  - `src-tauri/src/bin/myshell-mcp.rs`：`load_command_rules()`（读 JSON，失败用默认）+ ssh_exec 改为条件确认（`command_needs_confirmation()` 判定）+ 更新工具 description + 更新 SERVER_INSTRUCTIONS 的 SAFETY 段。
  - `src/components/SettingsPanel.tsx`：MCP 节新增"🛡️ 命令确认规则"子区块——confirm_unknown toggle + 黑名单 textarea + 白名单 textarea + 保存按钮 + 判定逻辑说明。
- **验证：** `cargo test --lib command_rules` 19/19 PASS。`cargo check` + `npx tsc --noEmit` PASS。手测：`ssh_exec "ps aux | grep sftp | grep -v grep"` **不再弹框**（10s 后返回 SSH 连接超时，证明直接执行未阻塞在对话框）。

## 五问重启检查（阶段 68）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 68 complete —— 命令确认规则实现并验证（ps 免确认通过）。 |
| 我要去哪里？ | 打包发布（含此功能 + test_connection 歧义修复，攒到 v2.2.0 或 v2.1.1）。 |
| 什么可能导致偏离？ | (1) 用户的命令模式不在默认黑名单（如自定义脚本删数据）→ confirm_unknown=false 时会放行（用户需自行加黑名单或开严格模式）；(2) 正则写错可能误报/漏报（用户可在 GUI 编辑修正）。 |
| 下一步最小可验证动作？ | 用户装新版，在设置里看到"命令确认规则"区块；让 AI 跑 ps（免确认）/ rm（弹框）。 |
| 目标是什么？ | ssh_exec 只在真正危险时打扰用户，只读命令顺畅执行；规则透明可配置。 |


### 阶段 69：ssh_exec 走 GUI tab 同步展示 + 自动启动 + 配置开关（2026-07-22）

- **需求：** MCP 调 ssh_exec 时自动启动 GUI，命令在 GUI 终端 tab 里同步展示（用户实时可见），输出回传给 AI。做成可配置开关。
- **架构：双向 IPC（MCP→GUI 发命令，GUI→MCP 回结果）+ Sentinel 标记法捕获 PTY 输出**
  - show_in_gui=true 时：MCP 通过 IPC 发 exec_in_tab → GUI 开 tab → 发 command + sentinel 到 PTY → 监听 ssh_output 流 → sentinel 出现时截取输出 → IPC 回传 → MCP 返回给 AI
  - show_in_gui=false 时：走原来的独立连接（headless，不变）
  - Sentinel 机制解决"PTY 输出无界流没有命令边界"问题：发 `command + "\necho __MCP_DONE_<uuid>__:$?\n"`，正则匹配 sentinel 行获取 exit_code
  - 自动启动：MCP 读不到端口文件 → spawn myshell.exe（同目录）→ 轮询等端口文件（最多 30s）
- **改动：**
  - `command_rules.rs`：CommandRules 加 `show_in_gui: bool`（默认 true）
  - `main.rs`：IPC listener 加 `exec_in_tab` action（生成 request_id → emit 事件 → oneshot channel 阻塞等前端回传）；新增 `mcp_exec_result` Tauri 命令（前端调它把结果送回等待中的 IPC 线程）；全局 PENDING_EXEC HashMap 关联 request_id ↔ oneshot::Sender
  - `api.ts`：mcpExecResult 封装；CommandRules 加 show_in_gui
  - `App.tsx`：mcp-gui-command listener 扩展处理 exec_in_tab（找/开 tab → sshSend 命令+sentinel → onSshOutput 累积 → 正则匹配 sentinel → mcpExecResult 回传）；tabsRef 解决闭包捕获旧 tabs 问题
  - `myshell-mcp.rs`：ssh_exec 分支加 show_in_gui 路径（exec_in_gui_tab + ensure_gui_running + TCP 客户端读结果）；失败时优雅回退 headless
  - `SettingsPanel.tsx`：命令确认规则区块加"ssh_exec 界面同步展示"toggle
- **验证：** `cargo check` PASS，`npx tsc --noEmit` PASS。功能手测待用户验证（需 dev GUI + MCP 联调）。

## 五问重启检查（阶段 69）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 69 complete —— show_in_gui 全链路实现并编译通过。 |
| 我要去哪里？ | dev GUI + MCP 联调验证：ssh_exec 走 GUI tab 展示 + sentinel 捕获输出。 |
| 什么可能导致偏离？ | (1) Sentinel 正则匹配可能因 PTY 回显/ANSI 码/多行命令偏差；(2) 新开 tab 后 SSH 连接建立需要时间，polling tabs 可能有延迟；(3) 并发 exec 调用（多个 AI 同时）可能争抢同一 tab。 |
| 下一步最小可验证动作？ | 用户开 dev GUI，让 AI 跑 `ssh_exec "ps aux"`，看 GUI tab 里命令出现 + AI 收到输出。 |
| 目标是什么？ | AI 执行的命令在 GUI 实时可见、输出回传，用户全程掌控；关闭开关则静默执行。 |


### 阶段 70：NSIS 安装器增加 mcp/cli 运行检查 + 修复重复打包（2026-07-22）

- **问题：** 用户升级到 v2.1.1 时安装报「无法打开要写入的文件：E:\Program Files\MyShell\myshell-mcp.exe」。根因：myshell-mcp.exe 常被各 AI 客户端（Claude Desktop / Cursor / ZCode 等）作为 MCP server 子进程常驻拉起，文件被占用导致 NSIS 无法覆盖写入；而安装器只在写文件时才报错（官方 CheckIfAppIsRunning 宏只检查主程序 myshell.exe）。
  - 实测当时机器上有 4 个 myshell-mcp.exe 进程在跑。
- **改动（`src-tauri/nsis/installer.nsi`，自定义 NSIS 模板）:
  1. Install section 在检查主程序后，追加检查 myshell-mcp.exe + myshell-cli.exe（复用 `CheckIfAppIsRunning` 宏）—— 安装/升级一开始就提示用户关闭，而不是写到一半才弹 Abort/Retry/Ignore。
  2. Uninstall section 同样追加 mcp + cli 运行检查，避免卸载时 Delete 失败留残文件。
  3. 卸载 Delete 主程序 + binaries 循环加 `/REBOOTOK`：即便文件仍被占用，也登记为重启删除，卸载不卡。
  4. 删除手写的冗余 `File "/oname=myshell-mcp.exe" "..\..\myshell-mcp.exe"` 与对应 `Delete "$INSTDIR\myshell-mcp.exe"`：经核 Tauri 按 workspace `[[bin]]` 目标已把 myshell-mcp.exe / myshell-cli.exe 自动注入 `{{#each binaries}}` 列表（生成模板里展开为绝对路径 File 指令），手写行属重复打包，且其相对路径 `..\..\` 解析到不存在的 `src-tauri/myshell-mcp.exe`，是隐患。
 5. 同步更新模板头部注释（MyShell 改动清单从 1 项扩到 3 项 + 维护须知）。
- **验证：** 源模板改动完成。需重新 `npm run tauri:build` 出新安装器才能端到端验证（检查宏触发 + 重复打包消除）。npx tsc / cargo check 不适用（纯 NSIS 模板改动，无 Rust/TS 代码变更）。

## 五问重启检查（阶段 70）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 70 complete —— installer.nsi 加 mcp/cli 运行检查 + 删重复打包，模板头部注释同步。 |
| 我要去哪里？ | 下次打包出安装器，验证升级时若 mcp 在跑会提示关闭而非写到一半失败。 |
| 什么可能导致偏离？ | (1) CheckIfAppIsRunning 只按镜像名匹配，若 mcp 被改名运行则查不到（极少见）；(2) /REBOOTOK 只是登记重启删除，用户不重启则文件仍在（可接受，卸载流程不卡）。 |
| 下一步最小可验证动作？ | 打包后，开着 AI 客户端（mcp 在跑）跑安装器，确认弹出「MyShell (MCP Server) 正在运行」提示而非写入失败。 |
| 目标是什么？ | 升级/卸载时不再因 mcp 占用卡在「无法打开要写入的文件」，提前提示用户关闭。 |

### 阶段 71：修�?ssh_exec 空输�?+ 新增 upload_project/download_project MCP 工具�?026-07-22�?
- **问题�?* ssh_exec �?show_in_gui 模式下返回空 stdout（exit_code 正确�?stdout 为空）；终端里能看到 sentinel 回显；无法一键上�?下载整个项目目录�?- **根因�?* src/App.tsx �?runExec 函数存在时序 bug —�?onSshOutput 订阅�?sshSend 命令发送之后才建立，导致快速命令的输出在订阅建立前就被消费完毕，outputBuf 里只�?sentinel 行。PS1 提示符剥离逻辑�?`command.startsWith(lines[0].trim())` 对带前缀的行（`[host]$ cmd`）也失败�?- **改动�?*
  1. `src/App.tsx` —�?�?onSshOutput 订阅移到 sshSend 之前；改�?`stdout.indexOf(command)` 定位回显边界；增�?CRLF→LF 规范化；尾部提示符行检�?  2. `src/App.tsx` —�?发�?sentinel 前先执行 `stty -echo` 隐藏终端回显，发完再 `stty echo` 恢复
  3. `src-tauri/src/bin/myshell-mcp.rs` —�?新增 `upload_project` 工具：本�?tar 打包（排�?.venv 等）�?SSH exec 管道直传 �?远程解压
  4. `src-tauri/src/bin/myshell-mcp.rs` —�?新增 `download_project` 工具：远�?sudo tar 打包 �?SFTP 下载 �?本地解压到附件目�?  5. `src-tauri/src/bin/myshell-mcp.rs` —�?upload_project/download_project 同时支持 ssh/sftp 连接类型（`find_connection(None)`�?- **验证�?* `npx tsc --noEmit` 通过；`cargo check --bin myshell-mcp` 通过；upload_project 成功上传 13MB sftpMonitor �?135.32.56.70:/opt/py；download_project 成功下载�?G:\桌面\myshell附件\

## 五问重启检查（阶段 71�?| 问题 | 答案 |
|------|------|
| 我在哪里�?| 阶段 71 complete —�?ssh_exec 空输出修�?+ upload_project/download_project 工具，端到端验证通过�?|
| 我要去哪里？ | 打包发布 v2.3.0（含 ✨新�?2 个工�?+ 🐛 修复 1 �?bug + 🛠�?优化 3 项）�?|
| 什么可能导致偏离？ | (1) 使用 ZCode 30s MCP 超时的大项目上传需确认�?2) 不同服务�?locale 设置差异可能影响 tar 中文文件名�?|
| 下一步最小可验证动作�?| 打包后跑一次完整安�?+ 上传/下载回归测试�?|
| 目标是什么？ | ssh_exec 可靠返回 stdout；AI 可一键部�?备份远程项目�?|

### 阶段 71：修复 ssh_exec 空输出 + 新增 upload_project/download_project MCP 工具（2026-07-22）

- **问题：** ssh_exec 在 show_in_gui 模式下返回空 stdout（exit_code 正确但 stdout 为空）；终端里能看到 sentinel 回显；无法一键上传/下载整个项目目录。
- **根因：** src/App.tsx 的 runExec 函数存在时序 bug —— onSshOutput 订阅在 sshSend 命令发送之后才建立，导致快速命令的输出在订阅建立前就被消费完毕，outputBuf 里只剩 sentinel 行。PS1 提示符剥离逻辑用 `command.startsWith(lines[0].trim())` 对带前缀的行（`[host]$ cmd`）也失败。
- **改动：**
  1. `src/App.tsx` —— 把 onSshOutput 订阅移到 sshSend 之前；改用 `stdout.indexOf(command)` 定位回显边界；增加 CRLF→LF 规范化；尾部提示符行检测
  2. `src/App.tsx` —— 发送 sentinel 前先执行 `stty -echo` 隐藏终端回显，发完再 `stty echo` 恢复
  3. `src-tauri/src/bin/myshell-mcp.rs` —— 新增 `upload_project` 工具：本地 tar 打包（排除 .venv 等）→ SSH exec 管道直传 → 远程解压
  4. `src-tauri/src/bin/myshell-mcp.rs` —— 新增 `download_project` 工具：远程 sudo tar 打包 → SFTP 下载 → 本地解压到附件目录
  5. `src-tauri/src/bin/myshell-mcp.rs` —— upload_project/download_project 同时支持 ssh/sftp 连接类型（`find_connection(None)`）
- **验证：** `npx tsc --noEmit` 通过；`cargo check --bin myshell-mcp` 通过；upload_project 成功上传 13MB sftpMonitor 到 135.32.56.70:/opt/py；download_project 成功下载到 G:\桌面\myshell附件\

## 五问重启检查（阶段 71）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 71 complete —— ssh_exec 空输出修复 + upload_project/download_project 工具，端到端验证通过。 |
| 我要去哪里？ | 打包发布 v2.3.0（含 ✨新增 2 个工具 + 🐛 修复 1 个 bug + 🛠️ 优化 3 项）。 |
| 什么可能导致偏离？ | (1) 使用 ZCode 30s MCP 超时的大项目上传需确认；(2) 不同服务器 locale 设置差异可能影响 tar 中文文件名。 |
| 下一步最小可验证动作？ | 打包后跑一次完整安装 + 上传/下载回归测试。 |
| 目标是什么？ | ssh_exec 可靠返回 stdout；AI 可一键部署/备份远程项目。 |


### 阶段 72：修复 MCP ssh_exec 不停开 tab + stdout 残留辅助命令回显（2026-07-22）

- **问题：** (1) 连续执行多次 MCP ssh_exec 时，GUI 不停打开重复的 terminal 标签页；(2) 返回的 stdout 里残留 stty -echo / echo __MCP_DONE__ / stty echo 辅助命令文字，且剥离 sentinel 后换行丢失导致输出粘连。
- **根因：**
  1. myshell-mcp.rs:1180 exec_in_gui_tab 调 send_gui_open_command(..., focus_existing=false)，GUI 端在 focus_existing=false 时跳过去重直接开新 tab → 每次执行都开新页
  2. src/App.tsx runExec 发送 sentinel 时往 PTY 发了三行辅助命令（stty -echo / echo __MCP_DONE__ / stty echo），这些命令的回显混入输出流，但 stdout 提取逻辑只剥离了 command 回显和 sentinel 结果行，没有剥离这三行辅助命令回显
- **改动：**
  1. src-tauri/src/bin/myshell-mcp.rs —— send_gui_open_command 第 4 参数 focus_existing 从 false 改为 true：已有该连接的 terminal tab 时复用而非开新页
  2. src/App.tsx 发送端 —— 去掉 stty -echo/stty echo 三行辅助命令（echo 时序不可靠，回显留噪音+^C），改为只发单行 echo sentinel:$?
  3. src/App.tsx 提取端 —— 截断式清洗：先剥离 ANSI 转义，再找第一个 echo __MCP_DONE_ 回显行从行首截断，丢弃 sentinel 命令及之后所有内容，只保留中间真实输出
- **验证：** npx tsc --noEmit 通过；cargo check --bin myshell-mcp 通过

## 五问重启检查（阶段 72）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 72 complete —— MCP ssh_exec 开 tab 去重（focus_existing=true）+ stdout 清洗（去 stty 三行改单行 sentinel + 截断式 ANSI 剥离），IPC 直连验证 stdout 干净、终端只剩 1 行 sentinel。 |
| 我要去哪里？ | 打包发布 v2.3.0（含阶段 71-72 的改动）。 |
| 什么可能导致偏离？ | focus_existing=true 后若 tab 状态非 connected（断连/重连中），exec 会落到等待新 tab 分支，需确认边界正常。 |
| 下一步最小可验证动作？ | 实跑连续 ssh_exec 确认只开一个 tab；确认返回 stdout 不含 stty/echo/MCP_DONE 文字。 |
| 目标是什么？ | MCP ssh_exec 复用单一 tab、返回干净 stdout。 |


### 阶段 73：Vault 安全加固 + MCP 终端噪音治理（2026-07-22）

- **安全加固：删除 MCP vault 密码 keyring 明文存储。** 旧模型：MCP 从 keyring 读 passphrase → 解锁 vault → 持有所有连接明文密码 → 可绕过 GUI 访问任意服务器（风险点）。新模型：MCP 完全不持有 DEK/passphrase，所有凭证访问经 GUI。
  - ssh_exec：走 GUI terminal tab（GUI 已解锁 vault，命令在 tab 内执行）
  - SFTP 工具：MCP 通过新 IPC get_connection_secrets 向 GUI 请求解密后的连接配置
  - 新增 db::get_all_connections_plaintext + MCP find_connection_id（纯文本查找，不需 DEK）
  - 删除 secrets.rs MCP passphrase 三函数 + main.rs 三 Tauri command + api.ts 三函数
  - 删除 SettingsPanel.tsx Vault 密码同步 UI 卡片 + CLI vault save-passphrase
- **终端噪音治理：** ssh_exec 的 sentinel 辅助命令（stty/echo __MCP_DONE__）在终端里留下噪音。方案：
  1. 发送端去掉 stty 三行，改为单行 echo sentinel:$?（上一阶段已做）
  2. TerminalPanel onSshOutput 渲染层新增行缓冲过滤器：含 __MCP_DONE_ 的行在写入 xterm 前丢弃，终端完全无噪音
  3. App.tsx stdout 提取端截断式清洗：ANSI 剥离 + 找第一个 echo __MCP_DONE_ 回显行截断
- **Bug 修复：** MCP ssh_exec 连续执行不停开新 tab（focus_existing: false → true，阶段 72）
- **验证：** npx tsc --noEmit 通过；cargo check 通过

## 五问重启检查（阶段 73）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 73 complete —— vault 安全加固（删 keyring 明文 + 强制走 GUI）+ 终端噪音治理（渲染层过滤），编译通过。 |
| 我要去哪里？ | 实测验证：安装后跑 ssh_exec/SFTP 确认 GUI 解锁后正常工作；GUI 未运行时报错提示正确。 |
| 什么可能导致偏离？ | SFTP 经 GUI IPC 解密密码是新的 IPC 协议，需确认 TCP 超时/并发无问题；keyring 中残留的旧 passphrase 不影响（已不读）。 |
| 下一步最小可验证动作？ | 打测试包安装，跑一次 ssh_exec + 一次 sftp_list 确认链路通。 |
| 目标是什么？ | MCP 无法绕过 GUI 访问服务器；终端无 sentinel 噪音。 |

### 阶段 74：修复 sz ./* 多文件下载三个 Bug（竞态挂起 + 100% 卡死 + rz 噪音）（2026-07-24）

- **Bug 1：首个文件完成后 UI 卡住，后续文件不下载。**
  - 根因（lost-offer race）：`ZmodemBridge.handleDownload` 用单一 `pendingResolve` 等待下一个 offer。但 zmodem.js 的 receive 端时序特殊：第一个文件的 ZEOF 一到，`_accept()` 内部就立即 `_send_ZRINIT()`（邀请 sender 发下一文件）**然后才** resolve `accept()`。也就是说在 `receiveFile` 还没返回、循环还没走到 `waitForNextOffer()` 之前，第二个文件的 ZFILE 可能已经到达并触发 `offer` 事件——此时 `pendingResolve` 仍是 null，offer 被静默丢弃，循环永远等不到 → 挂起。
  - `rz`（上传）方向不受影响：我们是 sender，节奏由本地 `for` 循环 + `send_offer()` 串行驱动，无竞态。
  - 修复（handleDownload）：① offer 队列 + `waitingForOffer` resolver 替代单一 `pendingResolve`，早到 offer 入队、drain 后再挂；② `session.start()` 返回的第一个 offer 与同步触发的 `offer` 事件是同一对象（`this._current_transfer`），dedupe 队列头部重复项；③ `sessionEnded` 标志 + `session_end` 释放等待中的 resolver，会话结束循环必然退出。

- **Bug 2：所有文件下载完（进度 100%）后 UI 卡几十秒才返回终端。**
  - 根因（fire-and-forget 写入 + 提前关闭句柄）：`receiveFile` 的 `input` 回调对 `szWriteChunk` 是 `.catch()` 不 await，写入 IPC 在后台飞。ZEOF 一到 `accept()` 立刻 resolve，`finally` 里 `await szClose(id)` 把 Rust map 里的句柄删掉——此时队列里还没落地的 `szWriteChunk` 全部报 "Unknown zmodem transfer id" → 文件被截断/末块丢失。lrzsz 看到自己发出的字节没被正确 ZEOF 确认，进入重传/超时窗口（几十秒）后才放弃 → UI 卡在 100% 面板。
  - 修复（receiveFile）：用 `pendingWrites: Promise<void>[]` 记录每个在途 `szWriteChunk`，`accept()` resolve 后 `await Promise.all(pendingWrites)` 排干再 `szClose`——保证文件精确到字节完整，lrzsz 立即收到正确 ZEOF ack 正常收尾。
  - Rust 侧 `sz_close` 无需改：它 remove 句柄后 `File` drop 自动 flush，问题只在前端"未排干就关闭"的时序。

- **Bug 3：sz 传输开始前终端多出一行 `rz`。**
  - 根因（lrzsz auto-start trigger）：lrzsz 的 `sz` 在发 ZMODEM 协议帧之前，会先往 stdout 打印 `rz\r\n`——这是 BBS 时代遗留的约定，让对端的 ZMODEM 感知终端自动拉起本地 `rz` 接收。MyShell 不需要它，但它出现在协议帧之前、Rust 还在 Normal 模式时，被 `append_capped` 当普通终端输出 flush 给了 xterm，于是用户看到一行莫名其妙的 `rz`。
  - 修复（ssh.rs `handle_incoming_data` Normal→Zmodem 切点）：新增 `strip_zmodem_autostart_noise(buffer)`，在 `find_zmodem_start` 命中、`flush_buffer` 前缀**之前**，从 buffer 尾部剥离 `rz` + 紧邻的 CR/LF。安全边界清晰：只在「确认有协议帧紧随其后」的切点调用，此时尾部的 `rz` 只可能是 lrzsz 触发串，不会误伤真实终端输出。`rz`（上传）方向不打印此串（只有 `sz` 发送方打印），故无副作用。

- **验证：** npx tsc --noEmit 通过；cargo check 通过（仅遗留 dead_code 警告）。

## 五问重启检查（阶段 74）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 74 complete —— sz 多文件下载三个 Bug（竞态 + 100% 卡死 + rz 噪音）修复，TS/Rust 编译均通过。 |
| 我要去哪里？ | 实测 `sz ./*` 批量下载确认：①所有文件连续完成 ②100% 后秒回终端 ③启动前无多余 `rz` 行；`rz` 上传回归确认未受影响。 |
| 什么可能导致偏离？ | dedupe 用 `===` 比较 offer 对象引用，依赖 zmodem.js 事件与 start() 返回同一 `_current_transfer` 实例（已核对源码 zsession.js:668-675 成立）。`strip_zmodem_autostart_noise` 只剥尾部 `rz`，若未来 lrzsz 改打印别的触发串需扩展。 |
| 下一步最小可验证动作？ | 远程 `sz ./*`（3+ 文件）跑一轮，观察启动瞬间终端不再出现裸 `rz` 行。 |
| 目标是什么？ | sz/rz 多文件批量传输稳定，无中途挂起、无收尾卡死、无启动噪音。 |

### 阶段 75：MCP ssh_exec 会话复用 + 原地重连 + 输出加固（2026-07-24）

- **问题 1（慢）：MCP ssh_exec 每次无脑 open_connection + 固定 sleep(5s)。**
  - 根因：`myshell-mcp.rs::exec_in_gui_tab` 在发 exec_in_tab 之前先发 open_connection 让 GUI 开标签页，然后死等 5 秒握手。但前端的 exec_in_tab handler 自己就会处理"找不到 connected tab 就开/连"——这 5 秒 + open_connection 在会话已存在时是纯浪费，还和 exec_in_tab 内部的连接逻辑产生竞态。
  - 修复：砍掉 `send_gui_open_command` + `tokio::time::sleep(5s)`，直接发 exec_in_tab。会话已存在时从 5-6s 降到 <1s。

- **问题 2（竞态）：open_connection 和 exec_in_tab 各触发一次 SSH 连接。**
  - 根因：open_connection 的 focus_existing 只查 status===connected 的 tab；5 秒后 SSH 还在 connecting → exec_in_tab 查不到 connected → 又触发一次 handleConnect → 同服务器连两次、开两个 tab。
  - 修复：问题 1 的改动已消除此竞态（不再提前 open_connection）。

- **问题 3（会话超时不重连）：服务器 TMOUT 踢断后，MCP 发命令开新 tab，旧的残留。**
  - 根因：exec_in_tab 只查 status==="connected"，找不到就开新 tab；已有的 disconnected tab 残留不清理。
  - 修复：exec_in_tab 改为三态查找——①connected 直接执行 ②disconnected/error 原地重连（复用 reconnectOne，不开新 tab）③不存在才开新 tab。

- **问题 3b（重连丢终端历史）：原 reconnectOne 改 tab.id（=sessionId），React key 变 → xterm 销毁重建 → 历史丢失。**
  - 根因：tab.id === sessionId，重连生成新 sessionId → tab.id 变 → key 变 → 组件树重建。
  - 修复：解耦 tab.id 与 sessionId——handleConnect 用独立生成的稳定 tabId（`tab-{ts}-{rand}`），不再等于 sessionId；reconnectOne 只更新 tab.sessionId 不改 tab.id。重连前从 terminalRegistry 读旧 xterm buffer（≤5000 行）存到 tab.reconnectSnapshot；新 TerminalPanel mount 时写回 + 打一行"[—— 以上为重连前历史 ——]"分隔符。

- **问题 4（颜色丢失）：MCP 命令输出没有颜色，和终端看到的不一致。**
  - 根因：`App.tsx` runExec 用 ANSI 剥离版（ansiCleaned）定位 sentinel helper 行后，直接用它覆盖了 stdout——所有颜色码丢失。本意只是定位，顺手覆盖是 bug。
  - 修复：新增 `stripFromAnsiPosition` 辅助函数——用 ansiCleaned 定位 helper 行的可见字符位置，然后映射回带 ANSI 的原始 stdout 做截断。AI 现在能拿到带颜色的输出（和用户终端一致）。

- **问题 5（输出炸弹）：大输出时 outputBuf 无上限 + 全文正则 = O(n²)，浏览器卡死。**
  - 根因：`outputBuf += decode(data)` 无上限累积；每收到一块数据做一次 `outputBuf.match(sentinelRe)` 全文扫描。
  - 修复：①outputBuf 加 4MB 上限（对齐 headless 路径），超限时保留尾部；②sentinel 匹配只扫尾部 2KB（sentinel 总在最后），匹配后映射回全缓冲 index。

- **问题 6（保险库锁定静默超时）：保险库锁定时 MCP 命令等 30 秒才报错。**
  - 根因：exec_in_tab 的 fresh-tab 轮询只查 status==="connected"，handleConnect 失败设 status="error" 但轮询不检测 error。
  - 修复：轮询里先查 status==="error" 的 tab，命中立刻报错（含 errorMessage，通常提示"请检查保险库是否解锁"），不再等 30s。

- **涉及文件（4 个，Rust 零改动）：**
  - `src-tauri/src/bin/myshell-mcp.rs`：exec_in_gui_tab 砍 open_connection + sleep
  - `src/App.tsx`：handleConnect/reconnectOne 解耦 tab.id + 快照；exec_in_tab 三态查找 + error 快速失败；颜色修复 + outputBuf 上限 + 尾部正则；新增 stripFromAnsiPosition
  - `src/api.ts`：Tab 接口加 reconnectSnapshot 字段
  - `src/components/TerminalPanel.tsx`：新增 tabId/reconnectSnapshot/onSnapshotConsumed props，mount 时恢复快照

- **验证：** npx tsc --noEmit 通过；cargo check（含 --bin myshell-mcp）通过（仅遗留 dead_code 警告）。

## 五问重启检查（阶段 75）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 75 complete —— MCP ssh_exec 六个问题（慢/竞态/不重连/颜色/炸弹/静默超时）全部修复，TS/Rust 编译均通过。 |
| 我要去哪里？ | 实测验证 7 个场景：①会话已存在连续 3 条命令每条 <1s ②无重复 tab ③TMOUT 超时后原地重连保留历史 ④ls --color 输出有颜色 ⑤seq 1000000 不卡死 ⑥保险库锁定快速报错 ⑦手动重连按钮保留历史。 |
| 什么可能导致偏离？ | tab.id 与 sessionId 解耦改动面大（handleConnect/reconnectOne/广播/handleCloseTab 全涉及），虽 tsc 通过但运行时可能有遗漏的 id===sessionId 假设；stripFromAnsiPosition 对复杂 ANSI 序列（256色、嵌套 OSC）的边界 case 需实测。 |
| 下一步最小可验证动作？ | MCP 连发两条命令到同一服务器，第二条应在 1 秒内返回（验证会话复用 + 速度提升）。 |
| 目标是什么？ | MCP ssh_exec 做到"和用户在终端里敲命令一样的体验"——快、不重复开 tab、超时自动重连保历史、输出有颜色。 |

### 阶段 76：MCP 任务结束后清理 GUI 标签页（已取消，2026-07-24）

- **需求：** MCP 任务完成后关闭本次会话开过的 GUI terminal tab。
- **结论：方案讨论后取消，不做自动关闭。** 原因：
  1. MCP 协议（stdio JSON-RPC）**没有"任务完成"信号**——server 只能感知 `tools/call` 来去和 stdin EOF。
  2. 实现过的两种方案都有硬伤：
     - **每次 exec 后关 tab**：破坏阶段 75 的会话复用 <1s 优化（每次都要重新握手 5-6s）。
     - **进程退出时一次性关**：对常驻型 AI client（Claude Desktop / Cursor）几乎不触发——它们跟 MCP server 同生命周期，只有关闭整个 app 才断开 stdin。tab 会一直留到关 app。
  3. 空闲超时方案（最后一次 exec 后 N 分钟无调用即关）和显式 close_session 工具方案也讨论过，用户决定**都不做**——保持 tab 开着由用户手动管理。
- **代码状态：** 已撤销全部相关改动，回到阶段 75 的干净状态。功能上不做任何自动关闭。
- **涉及文件（撤销后无净变化）：** myshell-mcp.rs / main.rs / App.tsx 三个文件的 close_tabs 相关代码全部移除。

## 五问重启检查（阶段 76）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 76 取消 —— 经过方案讨论，决定不做 MCP 自动关 tab，保持阶段 75 的会话复用体验，tab 由用户手动管理。 |
| 我要去哪里？ | 打包发布 v2.5.0（仅含阶段 75 的六项 ssh_exec 改造）。 |
| 什么可能导致偏离？ | 无——纯撤销，回到已验证的阶段 75 状态。 |
| 下一步最小可验证动作？ | git status 确认三个文件回到阶段 75 状态；tsc + cargo check 通过。 |
| 目标是什么？ | MCP ssh_exec 保持阶段 75 的体验：快、不重复开 tab、超时自动重连保历史、输出有颜色。tab 关闭交给用户。 |

### 阶段 77：修复 MCP 大输出命令误报超时（docker logs 卡 sentinel）（2026-07-25）

- **问题：** 用户报告 MCP ssh_exec 连 nas.ggbond.fun 失败，但实际 SSH 一直正常（test_connection 317ms 成功）。真正故障是 `sudo docker logs katelyatv-katelyatv-1 --tail 30` 这条命令触发 MCP 工具层 30s 超时——mcp.log 里能看到"部分输出"已经拿到了 docker logs 的完整内容（Cron job 日志），但结尾的 sentinel 结果行 `__MCP_DONE_xxx__:0` 30 秒内没被检测到。
- **根因 1（扫描窗口太小）：** 阶段 75 把 sentinel 扫描窗口从"全缓冲"缩到"尾部 2KB"以避免 O(n²)。但大输出场景下 PTY 流的最后一块 batch 可能远超 2KB（含 ANSI 颜色码 + prompt + 多行 buffered chunks），sentinel 结果行可能被扫不到。
- **根因 2（死等 sentinel 字面匹配）：** 原逻辑只有一条路径：要么 sentinel 结果行到达，要么硬超时。没有"命令实际已完成"的中间判断。`docker logs` 这种命令，输出全部送达后，sentinel 结果行因 PTY/SSH 流拥塞可能延迟很久才到——但此时命令早已完成，应该立即返回。
- **修复（三层完成策略）：**
  - **Tier 1（happy path）：** sentinel 结果行到达 → finalizeOutput(真实 exit code)。逻辑不变。
  - **Tier 2（idle fallback，新增）：** 监听器检测到 sentinel **命令回显**（`echo __MCP_DONE_xxx__:$?`）后，说明命令已提交，结果应该紧随其后。若此后 **5 秒无新数据**（输出静止），认为命令已完成（sentinel 结果卡在缓冲），立即 finalizeOutput(null)（exit code 未知→按 0 处理）。
  - **Tier 3（hard timeout）：** 用户 timeout 到期仍有持续输出（hang、interactive、tail -f）→ 报错 + 部分输出。保持原逻辑。
- **扫描窗口：** 2KB → 16KB，覆盖大输出 + prompt + ANSI 码的现实场景，性能无感。
- **重构：** 把原本内联的 stdout 提取逻辑（ANSI 清洗、command echo 剥离、helper echo 剥离、prompt 剥离）抽成 `finalizeOutput(exitCode)` 函数，Tier 1 和 Tier 2 共用，保证两条路径输出处理完全一致。
- **涉及文件（1 个）：** `src/App.tsx`：runExec 新增 lastDataAt/sawSentinelEcho/finalizeOutput；sentinel 扫描窗口 2KB→16KB；新增 idleCheck 5 秒兜底；重构 stdout 提取。
- **验证：** npx tsc --noEmit 通过。

## 五问重启检查（阶段 77）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 77 complete —— 修复 MCP 大输出命令误报超时（三层完成策略 + 16KB 扫描窗口 + 公共 finalizeOutput），TS 编译通过。 |
| 我要去哪里？ | 打测试包，实测 docker logs --tail 30 不再超时。 |
| 什么可能导致偏离？ | idle fallback 5 秒阈值：若网络抖动恰好造成 >5s 静默但命令还在跑（罕见），会提前返回。可接受——比死等 30s 强，且只发生在已看到 sentinel 命令回显之后。 |
| 下一步最小可验证动作？ | MCP 执行 `sudo docker logs <容器> --tail 30` 这类大输出命令，应在命令实际完成 + 5s 内返回（不再硬等 30s）。 |
| 目标是什么？ | MCP ssh_exec 大输出命令稳定返回——sentinel 到了用真实 exit code，sentinel 卡了用 idle 兜底，命令 hang 了用硬超时报错。 |

### 阶段 78：修复匿名统计上报被 CSP 拦截 + 重试死锁（2026-07-25）

- **问题：** 用户反馈 https://cloud.umami.is/analytics 收不到统计信息。
- **根因 1（CSP 拦截，主因）：** `tauri.conf.json` 的 CSP `connect-src` 只允许 `'self' ipc: http://ipc.localhost`，没有放行 `https://cloud.umami.is`。WebView2 严格遵守 CSP，直接拦截 `fetch("https://cloud.umami.is/api/send")`，连网络请求都发不出去。用 curl 测 endpoint 是通的（200 + `{"beep":"boop"}`），但 curl 不受 CSP 管辖——误导排查。
- **根因 2（重试死锁）：** App.tsx onAgree 回调里先调 `markVersionHandled(version)`（写 version stamp），再 `void reportVersion(...)`。若 fetch 失败，stamp 已写 → `isVersionReported` 返回 true → 永远不再触发上报。用户点了同意但事件从未送达，且无法自救。
- **根因 3（策略放大了 2 的影响）：** `checkReportNeeded` 原设计"每次版本升级都重新弹同意框"（hasConsent 恒返回 false）。即使修复根因 2，用户同意后失败，下次启动还会再弹窗——体验差。
- **修复：**
  - **CSP 白名单**：`connect-src` 加 `https://cloud.umami.is`（tauri.conf.json）。
  - **重试逻辑**：onAgree 不再立即 markVersionHandled，改由 reportVersion 在 fetch 成功后自己标记（usageStats.ts:121-125 已有此逻辑）。失败时不写 stamp → 下次启动 checkReportNeeded 返回 shouldReport=true → 重试。
  - **同意策略改为 ask-once**：checkReportNeeded 真正读 hasStatsConsent——同意过的用户版本升级时静默上报（含失败重试），不再弹窗；拒绝过的用户每个新版本重新问。
- **涉及文件（3 个）：**
  - `src-tauri/tauri.conf.json`：CSP connect-src 加 cloud.umami.is
  - `src/App.tsx`：onAgree 移除立即 markVersionHandled
  - `src/lib/usageStats.ts`：checkReportNeeded 读真实 hasConsent；注释对齐 ask-once 策略
- **验证：** npx tsc --noEmit 通过；endpoint 直连 200。

## 五问重启检查（阶段 78）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 78 complete —— 修复 CSP 拦截 + 重试死锁 + 同意策略，TS 编译通过。 |
| 我要去哪里？ | 打 v2.5.1 发布，用户升级后点同意，Umami 后台应开始收到 app_launch_v2.5.1 事件。 |
| 什么可能导致偏离？ | 老版本用户之前点的"同意"因 CSP 拦截从未送达，localStorage 里 myshell.statsConsent=agreed 已写但 version stamp 没写 → 升级到 v2.5.1 后会静默重试上报（不再弹窗），符合预期。若用户之前点过"拒绝"，statsConsent 未写，v2.5.1 首启会重新弹窗——也符合预期。 |
| 下一步最小可验证动作？ | 装 v2.5.1，首启点同意，1 分钟内 Umami 后台应出现 app_launch_v2.5.1 事件。 |
| 目标是什么？ | 匿名统计真正可达——CSP 放行、失败可重试、同意一次后静默不打扰。 |

### 阶段 79：MCP 调用等待保险库解锁（2026-07-25）

- **需求：** 当 AI 调用 myshell-mcp 工具时，如果 GUI 还没输入主密码，每隔 3 秒确认一次是否已解锁，30 秒还没解锁就提示用户。
- **原行为：** MCP 工具立即失败（"保险库未解锁" 或 "MyShell GUI 未运行"），用户来不及反应。
- **修复：**
  - **GUI 侧**：`main.rs` IPC 分发新增 `vault_status` action，返回 `{ok, initialized, unlocked}`——复用已有 `vault::is_initialized()` + `AppState.dek.is_some()` 判断。
  - **MCP 侧**：新增 `query_gui_vault_status(port)` 通过 localhost IPC 查询；新增 `wait_for_vault_unlocked()` 三分支逻辑：①GUI 未运行→立即报错 ②已解锁→Ok 立即返回 ③已锁定→每 3s 轮询一次、最多 10 次（30s），期间用户解锁了就放行，超时返回"请在 GUI 输入主密码"。
  - **闸门位置**：`call_tool` 入口（match name 前），只对需要凭据的工具（ssh_exec、sftp_*、test_connection 等）生效；`list_connections` / `screenshot_terminal` / `open_in_gui` 豁免（前者纯文本查询不需解锁，后两者会触发 GUI 自己的解锁流程）。
- **涉及文件（2 个）：** `src-tauri/src/main.rs`（vault_status IPC action）、`src-tauri/src/bin/myshell-mcp.rs`（query + wait + 闸门）。
- **验证：** cargo check --bin myshell / --bin myshell-mcp 均通过（仅遗留无关 dead_code warning）。

## 五问重启检查（阶段 79）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 79 complete —— MCP 调用需要凭据的工具时，保险库未解锁则轮询等待 30s，cargo check 通过。 |
| 我要去哪里？ | 打 v2.5.2 发布，实测：锁保险库 → AI 调 ssh_exec → 应在 GUI 提示用户解锁，解锁后工具继续执行。 |
| 什么可能导致偏离？ | 30s 超时是硬编码常量，若 AI client 自己的 tool 超时 < 30s（如 Claude Desktop 默认 60s 够用），可能在 MCP 还在等待时被 client 提前取消。可接受——取消后用户解锁，下次调用秒过。 |
| 下一步最小可验证动作？ | 锁保险库，让 AI 调 list_connections（应秒过）+ ssh_exec（应等待），在 GUI 输主密码后观察 ssh_exec 继续。 |
| 目标是什么？ | MCP 工具对未解锁状态友好——给用户 30s 反应窗口，不再立即失败；同时不阻塞纯查询工具。 |

### 阶段 80：修复 MCP ssh_exec 长耗时命令导致会话挂起（2026-07-25）

- **问题：** 用户反馈 MCP ssh_exec 通道在长耗时命令（pip install、sleep 60）下挂起，后续命令全部卡死，最终 AI client 杀掉 MCP 重启。
- **根因（PTY 架构固有限制 + 三个叠加问题）：**
  1. **PTY 是交互式 shell，命令顺序排队**。阶段 75 的"会话复用"让所有 ssh_exec 共用同一 PTY。前一条命令（pip install）还在跑时，下一条命令的字符被注入同一 PTY，排在 shell 输入缓冲里，要等 pip 结束 shell 才执行。
  2. **输出重定向让 idle 检测失效**。`pip install > /tmp/log 2>&1` 让 PTY 零输出，阶段 77 的 idle fallback 要求"看到 sentinel 命令回显 + 5s 无新数据"，但 shell 还在跑 pip，sentinel 命令根本没回显，idle 检测条件不满足，死等硬超时。
  3. **超时后 PTY 命令没被清理（最致命）**。MCP 超时后给 AI 返回错误，但 PTY 里的 pip 还在跑。下一条命令进来复用同一 session，字符注入到还在跑 pip 的 PTY——命令交错，sentinel 崩溃，恶性循环直到 MCP 被杀重启。
- **修复（A+B 双管齐下）：**
  - **A. 超时后发 Ctrl+C**：Tier 3 硬超时分支在返回错误前向 PTY 发 `\x03`，打断残留进程，恢复干净 prompt。
  - **B. 命令互斥锁**：新增 `mcpExecLocksRef: Set<connectionId>`，exec_in_tab 入口检查——同一连接前一条命令没完成时，立即拒绝新命令并返回明确提示（"用 nohup 后台执行 + 轮询日志"）。锁通过 `finishExec` 包装器在所有 11 个出口统一释放。
- **涉及文件（1 个）：** `src/App.tsx`：mcpExecLocksRef + finishExec 包装器 + 锁检查 + Ctrl+C 发送 + 7 处 mcpExecResult→finishExec 替换。
- **验证：** npx tsc --noEmit 通过。

## 五问重启检查（阶段 80）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 80 complete —— 修复 MCP ssh_exec 长耗时命令挂起（命令互斥锁 + 超时发 Ctrl+C），TS 编译通过。 |
| 我要去哪里？ | 打 v2.5.3 发布，实测：AI 连发两条命令到同一服务器，第二条应立即被拒（锁）；长命令超时后 PTY 应被 Ctrl+C 清干净。 |
| 什么可能导致偏离？ | 互斥锁改变了 AI 调用模式——AI 必须学会用 nohup 跑长任务。短期内 AI 可能反复试 ssh_exec 然后被拒，需要工具描述引导（本次未改 description，留待后续）。 |
| 下一步最小可验证动作？ | MCP 连发 `sleep 30; echo done`（timeout=5，应超时发 Ctrl+C）然后立刻发 `whoami`（应成功，证明 PTY 已清干净）。 |
| 目标是什么？ | MCP ssh_exec 在 AI 滥用（连发、长命令）下不再挂死——锁防止交错，Ctrl+C 防止残留。 |

### 阶段 81：AI 面板 exec_once 自动重连（2026-07-25）

- **问题：** GUI 底部「服务器信息」/ AI 巡检报错 `Open exec channel failed: Failed to open channel (ConnectFailed)`。
- **根因：** `ssh::exec_once`（AI 面板/服务器信息探针走的路径）没有重连能力。底层 SSH transport 已断（服务器 idle 超时、NAT、网络抖动），但 session 还在 `ssh_sessions` map 里，channel 打开就失败。之前只在 `ai.rs` 加了友好提示让用户手动重连 tab。
- **修复：** 给 `exec_once` 加自动重连+重试：
  - `SshSession` 新增 `config: Arc<ConnectionConfig>` 字段——connect 时已持有 config，直接存 Arc，零额外解密/拷贝。
  - 新增 `is_connection_dead(err)` 启发式判断（ConnectFailed / disconnect / channel open / Session not found 等）。
  - 新增 `open_exec_channel(state, sid)` 抽取 channel 打开逻辑（可重试）。
  - 新增 `reconnect_session(state, sid)`——用存储的 config 重新 dial+auth，替换 sessions map 里的 handle。
  - `exec_once` 改为：第一次 channel open 失败且判定为连接失效 → `reconnect_session` → 重试 `open_exec_channel` → 仍失败才报错。
- **边界说明：** 重连只替换 exec 用的 handle，不碰前端 PTY 的 reader task——旧 reader 会检测到断开发 ssh_closed，前端把 tab 标记 disconnected（正确，连接确实断过）。用户若要在 terminal 继续输入，仍需手动重连 tab（前端 reconnectOne）。本次只保证 AI 巡检/服务器信息探针的可用性。
- **涉及文件（1 个）：** `src-tauri/src/ssh.rs`：SshSession 加 config 字段；connect 存 config；新增 3 个辅助函数；exec_once 重连逻辑。
- **验证：** cargo check --bin myshell 通过。

## 五问重启检查（阶段 81）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 81 complete —— AI 面板 exec_once 自动重连+重试，cargo check 通过。 |
| 我要去哪里？ | 打 v2.5.4 发布，实测：打开一个 SSH tab，等服务器 idle 断开，点 AI 巡检——应自动重连成功而不是报 ConnectFailed。 |
| 什么可能导致偏离？ | 存 config 到 session 内存里多持有一份密码——可接受（session 生命周期内，connect 本来就有）。reconnect 只修 exec 路径，不修前端 PTY——若用户紧接着在 terminal 输入会发到旧死 handle，需手动重连 tab（已有功能）。 |
| 下一步最小可验证动作？ | SSH tab idle 断开后，AI 面板点「巡检」——应成功（后台自动重连），不再报 ConnectFailed。 |
| 目标是什么？ | AI 巡检对连接失效自愈——用户不用先手动重连 tab 才能巡检。 |

### 阶段 82：修复 MCP 高危命令重复弹窗（2026-07-25）

- **问题：** GUI 高危命令弹窗隔一会儿没点，会弹 Windows 弹窗，用户被迫点两次。
- **根因：** ssh_exec 的双路径设计：
  1. show_in_gui 路径先跑，GUI 弹第一个 React 确认框
  2. 用户没点 → GUI 侧 exec_in_tab 30s 超时 → MCP 收到错误 → **回退到 headless 路径**
  3. Headless 路径又调 confirm_dangerous_operation → 弹第二个 Windows MessageBoxW
  两个弹窗叠加，用户得点两次。
- **修复：** show_in_gui 失败回退 headless 时，加判断：
  - 如果 GUI **真正不可用**（错误含"未找到 myshell.exe"/"GUI 启动超时"/"无法定位"）→ 允许 headless 回退（含其 OS 弹窗）——因为 GUI 弹窗根本没机会显示
  - 如果 GUI **在线但 exec 失败**（用户没点确认/取消/session 错误）+ 命令需要确认 → **直接返回错误，不回退 headless**——避免第二个弹窗
  - 如果命令不需要确认（白名单）→ 照常回退 headless（不会弹窗）
- **涉及文件（1 个）：** `src-tauri/src/bin/myshell-mcp.rs`：ssh_exec handler 的 show_in_gui 失败分支加 gui_unreachable 判断 + 条件 return。
- **验证：** cargo check --bin myshell-mcp 通过。

## 五问重启检查（阶段 82）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 82 complete —— MCP 高危命令不再重复弹窗（GUI 在线失败不回退 headless 重弹），cargo check 通过。 |
| 我要去哪里？ | 打 v2.5.5 发布，实测：AI 发高危命令，GUI 弹窗出来后不点，等 30s 超时——应只弹一次，不再弹 Windows 窗。 |
| 什么可能导致偏离？ | gui_unreachable 的错误字符串匹配是启发式——如果 ensure_gui_running 的错误信息改了文案，匹配会失效。但有三个 OR 条件，容错性尚可。 |
| 下一步最小可验证动作？ | AI 发 `rm /tmp/test` 这种高危命令，GUI 弹窗出来后不点，等超时——观察是否只弹一次。 |
| 目标是什么？ | 高危命令只确认一次——GUI 弹窗是首选，没点就报错让 AI 重试，绝不弹第二个 OS 窗。 |

### 阶段 83：修复应用内检测不到更新（Gitee /releases/latest 指向错误版本）（2026-07-26）

- **问题：** 应用内"检查更新"始终检测不到新版本（v1.11.2 用户看不到 v2.x）。
- **根因：** Gitee 的 `/releases/latest` 接口不可靠——它持续返回 v1.11.2（2026-07-17 创建），而不是真正的最新版 v2.5.5。Gitee 的 "latest" 标记机制对通过 API 发布的 release 不可靠（v2.x 系列经 publish-gitee-release.mjs 发布，没拿到 latest 标记）。代码本身逻辑（平台筛 .exe/.deb、strategy auto/browser）都对，只是输入数据错了。
- **验证证据：**
  - `/releases/latest` → 返回 v1.11.2（错误）
  - `/releases?per_page=100` → 返回 27 个 release，按创建时间降序，v2.5.5 真实存在
- **修复：** `src-tauri/src/main.rs` `check_for_updates` 改用列表接口 `/releases?per_page=100&page=1`，客户端用已有的 `is_newer()` 在所有返回的 release 中选 `tag_name` 版本号最大的那个。完全不依赖 Gitee 的 latest 标记。
- **涉及文件（1 个）：**
  - `src-tauri/src/main.rs`：
    - 常量 `GITEE_LATEST_RELEASE` → `GITEE_RELEASES_LIST`（URL 改为列表接口）
    - `check_for_updates`：解析逻辑从"解析单个对象"改为"解析数组 + max_by(is_newer) 选最大版本"，后续字段（assets/created_at/body/html_url）从选中的 `latest_json` 取
- **验证：**
  - cargo check 通过（两个 dead_code warning 是预先存在的，与本次无关）
  - 端到端验证：列表接口返回 27 个 release，复刻的版本比较逻辑正确选出 v2.5.5；对 v1.11.2 → has_update=true，对 v2.5.5 → has_update=false
- **关联问题（不修）：** MCP/CLI 在 Linux 的可用性已确认——deb 包内含 `usr/bin/myshell` + `usr/bin/myshell-mcp` + `usr/bin/myshell-cli` 三个二进制，装到 /usr/bin/ 即在 PATH 中，共享 `~/.config/myshell/connections.db`，开箱可用。

### 阶段 84：连接高级选项——地址族 / 连接超时 / Keepalive（2026-07-26）

- **需求：** 主机有 AAAA 记录但 IPv6 黑洞时，SSH 连接会卡到回退 IPv4 才成功；且连接超时（10s）、keepalive（15s）此前是写死的全局值，无法按连接调整。
- **方案：** 给每个连接新增 3 个可选配置（仅 SSH/SFTP 生效，SFTP 复用 SSH 会话自动覆盖）：
  1. `address_family`：`auto`（默认）/ `ipv4` / `ipv6`
  2. `connect_timeout_secs`：`Option<u32>`，None = 10s
  3. `keepalive_interval_secs`：`Option<u32>`，None = 15s（keepalive_max 固定 3）
- **核心改动：** `dial_and_authenticate`（ssh.rs）是唯一拨号咽喉（connect / test_connection / 断线重连共用）。直连分支原来用 `client::connect`（russh 内部解析、无地址族控制），改为新增的 `dial_tcp` 辅助函数：`tokio::net::lookup_host` 解析 → 按 family 过滤 → `TcpStream::connect`（带每连接超时）→ 交给 `client::connect_stream`（复用代理路径已有的模式）。`with_connect_timeout` 重构为接受 `timeout: Duration` 参数。代理分支不变（代理目标由代理服务器解析）。
- **涉及文件（5 个）：**
  - `src-tauri/src/lib.rs`：ConnectionConfig 加 3 字段 + `default_address_family()`。
  - `src-tauri/src/db.rs`：CREATE TABLE + 幂等迁移 + INSERT/SELECT/row map/4 处结构体字面量（新列索引 21/22/23，get_deleted 的 deleted_at 顺移到 24）。
  - `src-tauri/src/ssh.rs`：`dial_tcp` + 每连接超时 + 每连接 keepalive；导入 SocketAddr/TcpStream。
  - `src/api.ts`：ConnectionConfig 接口镜像 3 字段。
  - `src/components/ConnectionDialog.tsx`：3 个 state + validate 范围校验（1-3600）+ buildConfig + 「高级选项」FieldGroup（仅 ssh/sftp 显示）。
- **范围外：** FTP 地址族/超时（suppaftp 内部解析，UI 已隐藏该组）、代理目标地址族、cipher/KEX、压缩、TERM。
- **验证：** `cargo check`（exit 0）+ `npx tsc --noEmit`（无错误）均通过。

## 五问重启检查（阶段 83）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 83 complete —— 应用内更新检测改用 Gitee 列表接口 + 客户端选最大版本，绕过 Gitee latest 标记不可靠问题，cargo check 通过，端到端验证选出 v2.5.5。 |
| 我要去哪里？ | 打包发布让旧版用户（v1.11.2 / 早期 v2.x）能通过应用内检查拿到 v2.5.5+。 |
| 什么可能导致偏离？ | per_page=100 上限：若以后 release 数 >100 且最大版本在第 101 条之后会漏——目前才 27 个，短期无忧；长期可加分页遍历。 |
| 下一步最小可验证动作？ | 旧版应用启动 → 检查更新 → 应弹窗提示 v2.5.5 + 下载按钮（Windows auto / Linux browser）。 |
| 目标是什么？ | 任何已发布旧版都能在应用内检测到真实最新版并被引导升级，不再卡在 v1.11.2。 |

## 五问重启检查（阶段 84）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 84 complete —— 连接新增地址族/连接超时/Keepalive 三个高级选项，Rust + TS 双侧类型检查通过。 |
| 我要去哪里？ | 实测：建 SSH 连接设「强制 IPv4」+ 超时 5 + keepalive 30 应连接成功；纯 IPv4 主机设「强制 IPv6」应报“无可用 IPv6 地址”。 |
| 什么可能导致偏离？ | `dial_tcp` 用 `lookup_host` 自行解析取代了 russh 内部解析——若某些主机名解析行为与 russh 原路径有差异（极少见），直连可能受影响；auto 模式保留全部地址、行为应等价。 |
| 下一步最小可验证动作？ | 起 `cargo tauri dev`，编辑一个 SSH 连接，确认「高级选项」组出现、保存后重开回显正确、强制 IPv6 对 IPv4 主机报错。 |
| 目标是什么？ | 让用户能绕开 IPv6 黑洞、按连接调超时/保活——选项默认值与原硬编码一致，不改既有连接行为。 |

### 阶段 85：主机密钥变更后无法连接——明确报错 + 右键重置信任（2026-07-27）

- **问题：** 用户服务器重装/重新生成密钥后，连接报含糊的 `SSH connect failed: Unknown server key`，且**无任何入口可恢复**——首次信任（TOFU）本身正常（日志反证：同端口新主机 192.168.3.30 首连成功），真正原因是 host key mismatch 被防中间人逻辑正确拒绝，但 russh 把 `check_server_key` 返回 false 统一显示成 "Unknown server key"，用户既看不懂也无法重新信任（db 无 delete、无命令、无 UI）。
- **根因定位：** 读运行时日志（`%AppData%/myshell/logs/`）确认 `[ssh] host key mismatch ... stored=SHA256:BWhBt3… got=SHA256:lUqnZI…`，即服务器主机密钥指纹变了。
- **方案（用户选定 A：明确报错 + 右键重置，保留 MITM 防护）：**
  1. `check_server_key` 检测到不匹配时，把（stored_fp, got_fp）写入 handler 上一个共享 `Arc<Mutex<Option<(String,String)>>>`。
  2. `dial_and_authenticate` 把拨号+握手包进 async 块统一产出 Result，失败时读该槽位：若是不匹配，用明确的中文错误替换 russh 的 "Unknown server key"（列出新旧指纹 + 指引右键重置）。russh 会消费 handler 且失败不归还，故用共享槽位旁路。
  3. 新增「重置主机密钥信任」能力：`db::delete_known_host(host, port)` → `reset_known_host` command（注册进 generate_handler!）→ `api.ts resetKnownHost` → Sidebar 连接 ⋯ 菜单项（仅 ssh/sftp 显示，ConfirmDialog 二次确认）。重置后下次连接重新走 TOFU。
- **涉及文件（5 个）：** `ssh.rs`（SshClient 加槽位 + check_server_key 记录 + dial async 块 + 错误细化）、`db.rs`（delete_known_host）、`main.rs`（reset_known_host command + 注册）、`api.ts`（resetKnownHost）、`Sidebar.tsx`（state + handler + ConnRow prop + 菜单项 + ConfirmDialog）。
- **验证：** `cargo check` Finished（仅既有 dead_code 警告）+ `npx tsc --noEmit` exit 0。

## 五问重启检查（阶段 85）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 85 complete —— 主机密钥变更不再卡死：报错明确列出新旧指纹，右键「重置主机密钥信任」可恢复，cargo check + tsc 通过。 |
| 我要去哪里？ | 实测：对 nas.ggbond.fun:22228 连接应看到明确的"主机密钥已变更"错误（非 Unknown server key）；右键重置后再连应成功（重新 TOFU）。 |
| 什么可能导致偏离？ | 错误细化依赖共享槽位在 connect 失败时被正确读取——若 russh 在 check_server_key 之外的环节先报错，槽位为空则回退原始错误（无害）。重置仅删 known_hosts 单行，不动凭据，可逆。 |
| 下一步最小可验证动作？ | 起 dev，连 nas.ggbond.fun → 看新错误文案；⋯ → 重置主机密钥信任 → 确认 → 重连成功。 |
| 目标是什么？ | 服务器合法换密钥后用户能自助恢复连接，同时不牺牲主机密钥变更的 MITM 防护（不自动接受）。 |
