use anyhow::Result;
use std::time::{Duration, SystemTime};
use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    path::{Path, PathBuf},
    ptr,
};
use windows::{
    core::{ComInterface, GUID, HSTRING, PCWSTR, PWSTR},
    Data::{Json::JsonObject, Xml::Dom::XmlDocument},
    Win32::{
        Networking::WinHttp::{
            WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
            WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
            WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
            WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize,
                IPersistFile,
                StructuredStorage::{
                    PropVariantClear, PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0,
                    PROPVARIANT_0_0_0,
                },
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            Variant::{VARENUM, VT_LPWSTR},
        },
        UI::Shell::{
            FOLDERID_Programs, IShellLinkW,
            PropertiesSystem::{IPropertyStore, PROPERTYKEY},
            SHGetKnownFolderPath, ShellLink, KNOWN_FOLDER_FLAG,
        },
    },
    UI::Notifications::{ToastNotification, ToastNotificationManager},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub release_url: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCache {
    pub current_version: String,
    pub last_checked_at: Option<SystemTime>,
    pub latest_version: Option<String>,
    pub latest_release_url: Option<String>,
    pub latest_published_at: Option<String>,
    pub notified_for_version: Option<String>,
    pub last_error: Option<String>,
}

impl UpdateCache {
    pub fn new(current_version: impl Into<String>) -> Self {
        Self {
            current_version: current_version.into(),
            last_checked_at: None,
            latest_version: None,
            latest_release_url: None,
            latest_published_at: None,
            notified_for_version: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckMode {
    Background,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckStatus {
    Skipped,
    UpToDate,
    UpdateAvailable(ReleaseInfo),
    Failed(String),
}

pub trait ReleaseSource {
    fn fetch_latest_release(&self) -> Result<ReleaseInfo>;
}

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const UPDATE_TTL: Duration = Duration::from_secs(60 * 60 * 24);
const GITHUB_RELEASES_LATEST_HOST: &str = "api.github.com";
const GITHUB_RELEASES_LATEST_PATH: &str = "/repos/Clickin/CBXShell-rs/releases/latest";
const APP_USER_MODEL_ID: &str = "Clickin.CBXShell.Manager";
const START_MENU_SHORTCUT_SUBDIR: &str = "CBXShell-rs";
const START_MENU_SHORTCUT_NAME: &str = "CBXShell Manager.lnk";
const TOAST_TAG: &str = "update-check";

const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

struct ComGuard(bool);

impl ComGuard {
    fn init() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)?;
        }
        Ok(Self(true))
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct WinHttpHandle(*mut c_void);

impl WinHttpHandle {
    fn new(handle: *mut c_void, context: &str) -> Result<Self> {
        if handle.is_null() {
            return Err(anyhow::anyhow!(
                "{} failed: {}",
                context,
                windows::core::Error::from_win32()
            ));
        }
        Ok(Self(handle))
    }
}

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub struct GithubReleaseSource;

impl ReleaseSource for GithubReleaseSource {
    fn fetch_latest_release(&self) -> Result<ReleaseInfo> {
        let body = fetch_release_response_body()?;
        parse_latest_release_response(&body)
    }
}

fn normalize_release_tag(tag: &str) -> Option<String> {
    let normalized = tag.trim().trim_start_matches(['v', 'V']);
    if normalized.is_empty() || parse_version_parts(normalized).is_none() {
        return None;
    }
    Some(normalized.to_string())
}

fn parse_version_parts(version: &str) -> Option<Vec<u32>> {
    let mut parts = Vec::new();
    for segment in version.split('.') {
        if segment.is_empty() || !segment.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        parts.push(segment.parse().ok()?);
    }
    Some(parts)
}

pub fn is_newer_release(current_version: &str, release_tag: &str) -> bool {
    let Some(current) = parse_version_parts(current_version.trim()) else {
        return false;
    };
    let Some(latest_tag) = normalize_release_tag(release_tag) else {
        return false;
    };
    let Some(latest) = parse_version_parts(&latest_tag) else {
        return false;
    };

    let max_len = current.len().max(latest.len());
    for index in 0..max_len {
        let current_part = *current.get(index).unwrap_or(&0);
        let latest_part = *latest.get(index).unwrap_or(&0);
        if latest_part > current_part {
            return true;
        }
        if latest_part < current_part {
            return false;
        }
    }

    false
}

pub fn should_skip_background_check(cache: &UpdateCache, now: SystemTime, ttl: Duration) -> bool {
    let Some(last_checked_at) = cache.last_checked_at else {
        return false;
    };
    let Ok(elapsed) = now.duration_since(last_checked_at) else {
        return false;
    };
    elapsed < ttl
}

pub fn parse_latest_release_response(body: &str) -> Result<ReleaseInfo> {
    let json = JsonObject::Parse(&HSTRING::from(body))
        .map_err(|e| anyhow::anyhow!("failed to parse GitHub release JSON: {}", e))?;

    if json
        .GetNamedBooleanOrDefault(&HSTRING::from("draft"), false)
        .map_err(|e| anyhow::anyhow!("failed to read draft flag: {}", e))?
    {
        return Err(anyhow::anyhow!("latest release payload is draft"));
    }

    if json
        .GetNamedBooleanOrDefault(&HSTRING::from("prerelease"), false)
        .map_err(|e| anyhow::anyhow!("failed to read prerelease flag: {}", e))?
    {
        return Err(anyhow::anyhow!("latest release payload is prerelease"));
    }

    let tag_name = json
        .GetNamedString(&HSTRING::from("tag_name"))
        .map_err(|e| anyhow::anyhow!("failed to read tag_name: {}", e))?
        .to_string_lossy();
    let version = normalize_release_tag(&tag_name)
        .ok_or_else(|| anyhow::anyhow!("invalid release tag '{}'", tag_name))?;

    let release_url = json
        .GetNamedString(&HSTRING::from("html_url"))
        .map_err(|e| anyhow::anyhow!("failed to read html_url: {}", e))?
        .to_string_lossy();
    let published_at = json
        .GetNamedString(&HSTRING::from("published_at"))
        .map_err(|e| anyhow::anyhow!("failed to read published_at: {}", e))?
        .to_string_lossy();

    Ok(ReleaseInfo {
        version,
        release_url,
        published_at,
    })
}

pub fn evaluate_update_check<S: ReleaseSource>(
    source: &S,
    mut cache: UpdateCache,
    current_version: &str,
    now: SystemTime,
    ttl: Duration,
    mode: UpdateCheckMode,
) -> (UpdateCache, UpdateCheckStatus) {
    cache.current_version = current_version.to_string();

    if mode == UpdateCheckMode::Background && should_skip_background_check(&cache, now, ttl) {
        return (cache, UpdateCheckStatus::Skipped);
    }

    match source.fetch_latest_release() {
        Ok(release) => {
            cache.last_checked_at = Some(now);
            cache.latest_version = Some(release.version.clone());
            cache.latest_release_url = Some(release.release_url.clone());
            cache.latest_published_at = Some(release.published_at.clone());
            cache.last_error = None;

            if is_newer_release(current_version, &release.version) {
                (cache, UpdateCheckStatus::UpdateAvailable(release))
            } else {
                (cache, UpdateCheckStatus::UpToDate)
            }
        }
        Err(error) => {
            let message = error.to_string();
            cache.last_checked_at = Some(now);
            cache.last_error = Some(message.clone());
            (cache, UpdateCheckStatus::Failed(message))
        }
    }
}

fn system_time_to_unix_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn unix_seconds_to_system_time(value: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(value)
}

pub fn cache_to_json(cache: &UpdateCache) -> Result<String> {
    let json = JsonObject::new()?;

    json.SetNamedValue(
        &HSTRING::from("current_version"),
        &windows::Data::Json::JsonValue::CreateStringValue(&HSTRING::from(&cache.current_version))?,
    )?;

    let last_checked_value = match cache.last_checked_at.and_then(system_time_to_unix_seconds) {
        Some(timestamp) => windows::Data::Json::JsonValue::CreateNumberValue(timestamp as f64)?,
        None => windows::Data::Json::JsonValue::CreateNullValue()?,
    };
    json.SetNamedValue(&HSTRING::from("last_checked_at"), &last_checked_value)?;

    let set_optional_string =
        |name: &str, value: &Option<String>, json: &JsonObject| -> windows::core::Result<()> {
            let json_value = match value {
                Some(value) => {
                    windows::Data::Json::JsonValue::CreateStringValue(&HSTRING::from(value))?
                }
                None => windows::Data::Json::JsonValue::CreateNullValue()?,
            };
            json.SetNamedValue(&HSTRING::from(name), &json_value)
        };

    set_optional_string("latest_version", &cache.latest_version, &json)?;
    set_optional_string("latest_release_url", &cache.latest_release_url, &json)?;
    set_optional_string("latest_published_at", &cache.latest_published_at, &json)?;
    set_optional_string("notified_for_version", &cache.notified_for_version, &json)?;
    set_optional_string("last_error", &cache.last_error, &json)?;

    Ok(json.Stringify()?.to_string_lossy())
}

pub fn cache_from_json(body: &str) -> Result<UpdateCache> {
    let json = JsonObject::Parse(&HSTRING::from(body))
        .map_err(|e| anyhow::anyhow!("failed to parse cache JSON: {}", e))?;

    let current_version = json
        .GetNamedString(&HSTRING::from("current_version"))
        .map_err(|e| anyhow::anyhow!("failed to read current_version: {}", e))?
        .to_string_lossy();

    let optional_string = |name: &str| -> Result<Option<String>> {
        if !json.HasKey(&HSTRING::from(name))? {
            return Ok(None);
        }
        let value = json.GetNamedValue(&HSTRING::from(name))?;
        if value.ValueType()? == windows::Data::Json::JsonValueType::Null {
            return Ok(None);
        }
        Ok(Some(
            json.GetNamedString(&HSTRING::from(name))?.to_string_lossy(),
        ))
    };

    let last_checked_at = if !json.HasKey(&HSTRING::from("last_checked_at"))? {
        None
    } else {
        let value = json.GetNamedValue(&HSTRING::from("last_checked_at"))?;
        if value.ValueType()? == windows::Data::Json::JsonValueType::Null {
            None
        } else {
            Some(unix_seconds_to_system_time(
                json.GetNamedNumber(&HSTRING::from("last_checked_at"))? as u64,
            ))
        }
    };

    Ok(UpdateCache {
        current_version,
        last_checked_at,
        latest_version: optional_string("latest_version")?,
        latest_release_url: optional_string("latest_release_url")?,
        latest_published_at: optional_string("latest_published_at")?,
        notified_for_version: optional_string("notified_for_version")?,
        last_error: optional_string("last_error")?,
    })
}

pub fn load_cache(_path: &Path, _current_version: &str) -> UpdateCache {
    let default_cache = UpdateCache::new(_current_version);
    let Ok(contents) = std::fs::read_to_string(_path) else {
        return default_cache;
    };
    let Ok(mut cache) = cache_from_json(&contents) else {
        return default_cache;
    };
    cache.current_version = _current_version.to_string();
    cache
}

pub fn save_cache(_path: &Path, _cache: &UpdateCache) -> Result<()> {
    if let Some(parent) = _path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(_path, cache_to_json(_cache)?)?;
    Ok(())
}

pub fn run_background_check() -> Result<()> {
    let _com = ComGuard::init()?;
    let cache_path = default_cache_path();
    let source = GithubReleaseSource;
    let cache = load_cache(&cache_path, CURRENT_VERSION);
    let (mut updated_cache, status) = evaluate_update_check(
        &source,
        cache,
        CURRENT_VERSION,
        SystemTime::now(),
        UPDATE_TTL,
        UpdateCheckMode::Background,
    );
    save_cache(&cache_path, &updated_cache)?;

    if let UpdateCheckStatus::UpdateAvailable(release) = status {
        if updated_cache.notified_for_version.as_deref() != Some(release.version.as_str()) {
            if let Err(error) = show_update_toast(&release) {
                updated_cache.last_error = Some(error.to_string());
                save_cache(&cache_path, &updated_cache)?;
                return Ok(());
            }

            updated_cache.notified_for_version = Some(release.version);
            save_cache(&cache_path, &updated_cache)?;
        }
    }

    Ok(())
}

pub fn run_manual_check() -> Result<()> {
    let _com = ComGuard::init()?;
    let cache_path = default_cache_path();
    let source = GithubReleaseSource;
    let cache = load_cache(&cache_path, CURRENT_VERSION);
    let (updated_cache, status) = evaluate_update_check(
        &source,
        cache,
        CURRENT_VERSION,
        SystemTime::now(),
        UPDATE_TTL,
        UpdateCheckMode::Manual,
    );
    save_cache(&cache_path, &updated_cache)?;

    match status {
        UpdateCheckStatus::UpToDate | UpdateCheckStatus::Skipped => {
            super::utils::show_success(
                "CBXShell Manager",
                &format!("CBXShell-rs v{} is up to date.", CURRENT_VERSION),
            );
        }
        UpdateCheckStatus::UpdateAvailable(release) => {
            if super::utils::prompt_open_release(CURRENT_VERSION, &release.version) {
                super::utils::open_url(&release.release_url)?;
            }
        }
        UpdateCheckStatus::Failed(error) => {
            super::utils::show_error(
                "CBXShell Update Check Failed",
                &format!("Unable to check for updates.\n\n{}", error),
            );
        }
    }

    Ok(())
}

fn default_cache_path() -> PathBuf {
    let base_dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base_dir.join("CBXShell").join("update-cache.json")
}

fn fetch_release_response_body() -> Result<String> {
    unsafe {
        let agent = to_utf16("CBXShell-rs update checker");
        let session = WinHttpHandle::new(
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "WinHttpOpen",
        )?;

        WinHttpSetTimeouts(session.0, 5_000, 5_000, 5_000, 5_000)?;

        let host = to_utf16(GITHUB_RELEASES_LATEST_HOST);
        let connection = WinHttpHandle::new(
            WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0),
            "WinHttpConnect",
        )?;

        let method = to_utf16("GET");
        let path = to_utf16(GITHUB_RELEASES_LATEST_PATH);
        let request = WinHttpHandle::new(
            WinHttpOpenRequest(
                connection.0,
                PCWSTR(method.as_ptr()),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;

        let headers = format!(
            "Accept: application/vnd.github+json\r\nUser-Agent: CBXShell-rs/{}\r\n",
            CURRENT_VERSION
        );
        let headers = headers.encode_utf16().collect::<Vec<_>>();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, ptr::null_mut())?;

        let mut status_code: u32 = 0;
        let mut status_code_len = std::mem::size_of::<u32>() as u32;
        let mut header_index = 0;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status_code as *mut u32).cast()),
            &mut status_code_len,
            &mut header_index,
        )?;

        if status_code != 200 {
            return Err(anyhow::anyhow!("GitHub API returned HTTP {}", status_code));
        }

        let mut body = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let mut bytes_read = 0u32;
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut bytes_read,
            )?;
            if bytes_read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..bytes_read as usize]);
        }

        Ok(String::from_utf8(body)
            .map_err(|e| anyhow::anyhow!("GitHub API response was not UTF-8: {}", e))?)
    }
}

fn show_update_toast(release: &ReleaseInfo) -> Result<()> {
    ensure_start_menu_shortcut()?;

    let toast_xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>CBXShell-rs update available</text><text>Version {version} is available. Download the latest NSIS installer from GitHub Releases.</text></binding></visual><actions><action content=\"Open release\" activationType=\"protocol\" arguments=\"{url}\"/></actions></toast>",
        version = xml_escape(&release.version),
        url = xml_escape(&release.release_url),
    );

    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(toast_xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    toast.SetTag(&HSTRING::from(TOAST_TAG))?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))?;
    notifier.Show(&toast)?;
    Ok(())
}

fn ensure_start_menu_shortcut() -> Result<()> {
    let shortcut_path = start_menu_shortcut_path()?;
    if let Some(parent) = shortcut_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let current_exe = std::env::current_exe()?;
    let current_exe_wide = to_utf16_path(&current_exe);
    let shortcut_path_wide = to_utf16_path(&shortcut_path);
    let description = to_utf16("CBXShell Manager");

    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell_link.SetPath(PCWSTR(current_exe_wide.as_ptr()))?;
        shell_link.SetDescription(PCWSTR(description.as_ptr()))?;

        let property_store: IPropertyStore = shell_link.cast()?;
        let mut app_id = propvariant_from_string(APP_USER_MODEL_ID)?;
        property_store.SetValue(&PKEY_APP_USER_MODEL_ID, &app_id)?;
        property_store.Commit()?;
        PropVariantClear(&mut app_id)?;

        let persist_file: IPersistFile = shell_link.cast()?;
        persist_file.Save(PCWSTR(shortcut_path_wide.as_ptr()), true)?;
    }

    Ok(())
}

fn start_menu_shortcut_path() -> Result<PathBuf> {
    unsafe {
        let raw_path = SHGetKnownFolderPath(&FOLDERID_Programs, KNOWN_FOLDER_FLAG(0), None)?;
        let programs_dir = raw_path.to_string()?;
        CoTaskMemFree(Some(raw_path.0 as _));
        Ok(PathBuf::from(programs_dir)
            .join(START_MENU_SHORTCUT_SUBDIR)
            .join(START_MENU_SHORTCUT_NAME))
    }
}

fn propvariant_from_string(value: &str) -> Result<PROPVARIANT> {
    let mut wide = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    unsafe {
        let allocation = CoTaskMemAlloc(wide.len() * std::mem::size_of::<u16>());
        if allocation.is_null() {
            return Err(anyhow::anyhow!(
                "CoTaskMemAlloc failed for PROPVARIANT string"
            ));
        }
        ptr::copy_nonoverlapping(wide.as_ptr(), allocation.cast::<u16>(), wide.len());
        wide.fill(0);

        Ok(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VARENUM(VT_LPWSTR.0),
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        pwszVal: PWSTR(allocation.cast::<u16>()),
                    },
                }),
            },
        })
    }
}

fn to_utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn to_utf16_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        release: Option<ReleaseInfo>,
        error_message: Option<&'static str>,
    }

    impl ReleaseSource for FakeSource {
        fn fetch_latest_release(&self) -> Result<ReleaseInfo> {
            match (&self.release, self.error_message) {
                (Some(release), None) => Ok(release.clone()),
                (None, Some(message)) => Err(anyhow::anyhow!(message)),
                _ => Err(anyhow::anyhow!("invalid fake source state")),
            }
        }
    }

    #[test]
    fn compares_versions_after_stripping_v_prefix() {
        assert!(is_newer_release("5.2.0", "v5.2.1"));
        assert!(!is_newer_release("5.2.0", "v5.2.0"));
        assert!(!is_newer_release("5.2.1", "v5.2.0"));
    }

    #[test]
    fn skips_background_check_when_cache_is_fresh() {
        let mut cache = UpdateCache::new("5.2.0");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 10);
        cache.last_checked_at = Some(now - Duration::from_secs(60));

        assert!(should_skip_background_check(
            &cache,
            now,
            Duration::from_secs(86_400)
        ));
    }

    #[test]
    fn does_not_skip_background_check_when_cache_is_stale() {
        let mut cache = UpdateCache::new("5.2.0");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 10);
        cache.last_checked_at = Some(now - Duration::from_secs(86_400 + 1));

        assert!(!should_skip_background_check(
            &cache,
            now,
            Duration::from_secs(86_400)
        ));
    }

    #[test]
    fn parses_latest_release_payload() {
        let body = r#"{
            "tag_name": "v5.2.3",
            "html_url": "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.3",
            "published_at": "2026-04-11T12:00:00Z",
            "draft": false,
            "prerelease": false
        }"#;

        let release = parse_latest_release_response(body).expect("release should parse");
        assert_eq!(release.version, "5.2.3");
        assert_eq!(
            release.release_url,
            "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.3"
        );
        assert_eq!(release.published_at, "2026-04-11T12:00:00Z");
    }

    #[test]
    fn rejects_prerelease_payload() {
        let body = r#"{
            "tag_name": "v5.3.0-rc1",
            "html_url": "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.3.0-rc1",
            "published_at": "2026-04-11T12:00:00Z",
            "draft": false,
            "prerelease": true
        }"#;

        assert!(parse_latest_release_response(body).is_err());
    }

    #[test]
    fn background_failure_preserves_previous_release_metadata() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 10);
        let mut cache = UpdateCache::new("5.2.0");
        cache.latest_version = Some("5.2.1".to_string());
        cache.latest_release_url =
            Some("https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.1".to_string());
        cache.latest_published_at = Some("2026-04-10T00:00:00Z".to_string());

        let source = FakeSource {
            release: None,
            error_message: Some("network down"),
        };

        let (updated_cache, status) = evaluate_update_check(
            &source,
            cache,
            "5.2.0",
            now,
            Duration::from_secs(86_400),
            UpdateCheckMode::Background,
        );

        assert_eq!(
            status,
            UpdateCheckStatus::Failed("network down".to_string())
        );
        assert_eq!(updated_cache.latest_version.as_deref(), Some("5.2.1"));
        assert_eq!(
            updated_cache.latest_release_url.as_deref(),
            Some("https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.1")
        );
        assert_eq!(
            updated_cache.latest_published_at.as_deref(),
            Some("2026-04-10T00:00:00Z")
        );
        assert_eq!(updated_cache.last_error.as_deref(), Some("network down"));
        assert_eq!(updated_cache.last_checked_at, Some(now));
    }

    #[test]
    fn manual_check_reports_update_when_release_is_newer() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 10);
        let cache = UpdateCache::new("5.2.0");
        let release = ReleaseInfo {
            version: "5.2.3".to_string(),
            release_url: "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.3".to_string(),
            published_at: "2026-04-11T12:00:00Z".to_string(),
        };
        let source = FakeSource {
            release: Some(release.clone()),
            error_message: None,
        };

        let (updated_cache, status) = evaluate_update_check(
            &source,
            cache,
            "5.2.0",
            now,
            Duration::from_secs(86_400),
            UpdateCheckMode::Manual,
        );

        assert_eq!(status, UpdateCheckStatus::UpdateAvailable(release));
        assert_eq!(updated_cache.latest_version.as_deref(), Some("5.2.3"));
        assert_eq!(updated_cache.last_checked_at, Some(now));
        assert!(updated_cache.last_error.is_none());
    }

    #[test]
    fn cache_round_trips_through_json() {
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_234_567);
        let mut cache = UpdateCache::new("5.2.0");
        cache.last_checked_at = Some(timestamp);
        cache.latest_version = Some("5.2.3".to_string());
        cache.latest_release_url =
            Some("https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.3".to_string());
        cache.latest_published_at = Some("2026-04-11T12:00:00Z".to_string());
        cache.notified_for_version = Some("5.2.3".to_string());
        cache.last_error = Some("timeout".to_string());

        let serialized = cache_to_json(&cache).expect("cache should serialize");
        let deserialized = cache_from_json(&serialized).expect("cache should deserialize");

        assert_eq!(deserialized.current_version, "5.2.0");
        assert_eq!(deserialized.last_checked_at, Some(timestamp));
        assert_eq!(deserialized.latest_version.as_deref(), Some("5.2.3"));
        assert_eq!(
            deserialized.latest_release_url.as_deref(),
            Some("https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.3")
        );
        assert_eq!(
            deserialized.latest_published_at.as_deref(),
            Some("2026-04-11T12:00:00Z")
        );
        assert_eq!(deserialized.notified_for_version.as_deref(), Some("5.2.3"));
        assert_eq!(deserialized.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn cache_file_round_trip_preserves_latest_version() {
        let unique_name = format!(
            "cbxshell-update-cache-test-{}.json",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique_name);

        let mut cache = UpdateCache::new("5.2.0");
        cache.latest_version = Some("5.2.4".to_string());
        cache.last_error = Some("none".to_string());

        save_cache(&path, &cache).expect("cache should save");
        let loaded = load_cache(&path, "5.2.0");

        assert_eq!(loaded.current_version, "5.2.0");
        assert_eq!(loaded.latest_version.as_deref(), Some("5.2.4"));
        assert_eq!(loaded.last_error.as_deref(), Some("none"));

        let _ = std::fs::remove_file(path);
    }
}
