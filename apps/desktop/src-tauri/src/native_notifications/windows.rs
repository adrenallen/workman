//! Windows toasts with Workman's own identity, durable tags, and retained click handlers.
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationSetting, ToastDismissalReason, ToastNotification, ToastNotificationManager,
    },
    Win32::{
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoIncrementMTAUsage,
                CoInitializeEx, CoUninitialize, IPersistFile, STGM_READWRITE,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Variant::VT_LPWSTR,
        },
        UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, ShellLink},
    },
    core::{HSTRING, Interface, PCWSTR, PWSTR},
};

const GROUP: &str = "workman";

#[derive(Clone, Default)]
pub struct Backend {
    shown: Arc<Mutex<HashMap<i64, Delivered>>>,
    registered: Arc<Mutex<bool>>,
}

struct Delivered {
    toast: ToastNotification,
    activated: i64,
    dismissed: i64,
}

impl Drop for Delivered {
    fn drop(&mut self) {
        let _ = self.toast.RemoveActivated(self.activated);
        let _ = self.toast.RemoveDismissed(self.dismissed);
    }
}

// COM initialization must be paired on the same blocking worker thread.
struct Apartment;
impl Apartment {
    fn new() -> Result<Self, String> {
        // Toast callbacks outlive individual blocking commands. Keep the process MTA alive,
        // just as windows-core's activation factory does when initializing WinRT on demand.
        static MTA: OnceLock<Result<(), String>> = OnceLock::new();
        MTA.get_or_init(|| {
            unsafe { CoIncrementMTAUsage() }
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .clone()?;
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|error| error.to_string())?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

impl Backend {
    pub fn prepare(&self, app_id: &str, name: &str, executable: &Path) -> Result<(), String> {
        let _apartment = Apartment::new().map_err(|error| error.to_string())?;
        let mut registered = self.registered.lock().map_err(|error| error.to_string())?;
        if *registered {
            return Ok(());
        }
        let programs =
            std::env::var_os("APPDATA").ok_or("Windows did not provide the Start menu location")?;
        let directory = Path::new(&programs).join("Microsoft/Windows/Start Menu/Programs");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        // Register Workman's identity on its Start shortcut. Preserve an existing shortcut's
        // target, arguments, and icon when only adding the notification identity.
        let shortcut_path = directory.join(format!("{name}.lnk"));
        let executable = HSTRING::from(executable.as_os_str());
        let destination = HSTRING::from(shortcut_path.as_os_str());
        let description = HSTRING::from(name);
        let mut app_id_wide: Vec<u16> = app_id.encode_utf16().chain(Some(0)).collect();
        // SetValue copies this borrowed string; PROPVARIANT has no destructor and must not free it.
        let value = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    Anonymous: PROPVARIANT_0_0_0 {
                        pwszVal: PWSTR(app_id_wide.as_mut_ptr()),
                    },
                    ..Default::default()
                }),
            },
        };
        unsafe {
            let shortcut: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| error.to_string())?;
            let file: IPersistFile = shortcut.cast().map_err(|error| error.to_string())?;
            if shortcut_path.exists() {
                file.Load(PCWSTR(destination.as_ptr()), STGM_READWRITE)
                    .map_err(|error| error.to_string())?;
            } else {
                shortcut
                    .SetPath(PCWSTR(executable.as_ptr()))
                    .map_err(|error| error.to_string())?;
                shortcut
                    .SetDescription(PCWSTR(description.as_ptr()))
                    .map_err(|error| error.to_string())?;
            }
            let properties: IPropertyStore = shortcut.cast().map_err(|error| error.to_string())?;
            properties
                .SetValue(&PKEY_AppUserModel_ID, &value)
                .map_err(|error| error.to_string())?;
            properties.Commit().map_err(|error| error.to_string())?;
            file.Save(PCWSTR(destination.as_ptr()), true)
                .map_err(|error| error.to_string())?;
        }
        *registered = true;
        Ok(())
    }

    pub fn permission(&self, app_id: &str) -> Result<bool, String> {
        let _apartment = Apartment::new().map_err(|error| error.to_string())?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
            .map_err(|error| error.to_string())?;
        notifier
            .Setting()
            .map(|setting| setting == NotificationSetting::Enabled)
            .map_err(|error| error.to_string())
    }

    pub fn show(
        &self,
        app_id: &str,
        id: i64,
        title: &str,
        body: &str,
        on_open: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), String> {
        let _apartment = Apartment::new().map_err(|error| error.to_string())?;
        let document = XmlDocument::new().map_err(|error| error.to_string())?;
        document.LoadXml(&HSTRING::from("<toast><visual><binding template=\"ToastGeneric\"><text/><text/></binding></visual></toast>")).map_err(|error| error.to_string())?;
        let texts = document
            .GetElementsByTagName(&HSTRING::from("text"))
            .map_err(|error| error.to_string())?;
        for (index, value) in [title, body].iter().enumerate() {
            let node = document
                .CreateTextNode(&HSTRING::from(*value))
                .map_err(|error| error.to_string())?;
            texts
                .Item(index as u32)
                .and_then(|item| item.AppendChild(&node))
                .map_err(|error| error.to_string())?;
        }
        let toast = ToastNotification::CreateToastNotification(&document)
            .map_err(|error| error.to_string())?;
        toast
            .SetTag(&HSTRING::from(tag(id)))
            .map_err(|error| error.to_string())?;
        toast
            .SetGroup(&HSTRING::from(GROUP))
            .map_err(|error| error.to_string())?;
        let shown = Arc::downgrade(&self.shown);
        let activated = toast
            .Activated(&TypedEventHandler::new(
                move |sender: windows::core::Ref<'_, ToastNotification>, _| {
                    on_open();
                    if let Some(shown) = shown.upgrade() {
                        remove_matching(&shown, id, sender.as_ref());
                    }
                    Ok(())
                },
            ))
            .map_err(|error| error.to_string())?;
        let shown = Arc::downgrade(&self.shown);
        let dismissed = toast
            .Dismissed(&TypedEventHandler::new(
                move |sender: windows::core::Ref<'_, ToastNotification>,
                      args: windows::core::Ref<
                    '_,
                    windows::UI::Notifications::ToastDismissedEventArgs,
                >| {
                    // Timing out hides the banner but leaves it in Notification Center. Retain activation
                    // until the user clicks/clears it or Workman marks the matching item read.
                    if args.as_ref().and_then(|args| args.Reason().ok())
                        != Some(ToastDismissalReason::TimedOut)
                        && let Some(shown) = shown.upgrade()
                    {
                        remove_matching(&shown, id, sender.as_ref());
                    }
                    Ok(())
                },
            ))
            .map_err(|error| error.to_string())?;
        self.shown
            .lock()
            .map_err(|error| error.to_string())?
            .insert(
                id,
                Delivered {
                    toast: toast.clone(),
                    activated,
                    dismissed,
                },
            );
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
            .map_err(|error| error.to_string())?;
        if let Err(error) = notifier.Show(&toast) {
            self.shown
                .lock()
                .map_err(|error| error.to_string())?
                .remove(&id);
            return Err(error.to_string());
        }
        Ok(())
    }

    pub fn dismiss(&self, app_id: &str, ids: &[i64]) -> Result<(), String> {
        let _apartment = Apartment::new().map_err(|error| error.to_string())?;
        let app_id = HSTRING::from(app_id);
        let history = ToastNotificationManager::History().map_err(|error| error.to_string())?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)
            .map_err(|error| error.to_string())?;
        for id in ids {
            let toast = self
                .shown
                .lock()
                .map_err(|error| error.to_string())?
                .get(id)
                .map(|item| item.toast.clone());
            if let Some(toast) = toast {
                notifier.Hide(&toast).map_err(|error| error.to_string())?;
            }
            history
                .RemoveGroupedTagWithId(&HSTRING::from(tag(*id)), &HSTRING::from(GROUP), &app_id)
                .map_err(|error| error.to_string())?;
            self.shown
                .lock()
                .map_err(|error| error.to_string())?
                .remove(id);
        }
        Ok(())
    }
}

// Windows limits toast tags to 16 characters; a hexadecimal i64 remains within that limit.
fn tag(id: i64) -> String {
    format!("{id:x}")
}

fn remove_matching(
    shown: &Mutex<HashMap<i64, Delivered>>,
    id: i64,
    toast: Option<&ToastNotification>,
) {
    if let Ok(mut shown) = shown.lock()
        && shown
            .get(&id)
            .is_some_and(|item| Some(&item.toast) == toast)
    {
        shown.remove(&id);
    }
}
