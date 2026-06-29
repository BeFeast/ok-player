using System;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace OkPlayer.App.Services;

/// <summary>
/// Per-user (HKCU) file-association management for the unpackaged app. Registers a ProgID + an
/// "Applications\OkPlayer.exe" entry + Default-Apps capabilities, and assigns/unassigns OK Player for
/// individual extensions. <see cref="Assign"/> writes the legacy per-user default ProgID pointer
/// (HKCU\Software\Classes\&lt;ext&gt;), which makes a double-click open OK Player whenever Windows has no
/// hash-protected UserChoice for the type — the common case for media extensions (the same approach mpv.net
/// and MPC-HC take; no admin, no hash forgery). When Windows HAS hash-pinned the type to another app
/// (<see cref="HasForeignUserChoice"/>), only the user can switch it, via Settings → Default apps
/// (<see cref="OpenWindowsDefaultApps"/>). No admin rights are needed.
/// </summary>
public sealed class FileAssociationService
{
    private const string ProgId = "OkPlayer.MediaFile";
    private const string AppRegName = "OK Player";
    private const string CapabilitiesKey = @"Software\OkPlayer\Capabilities";
    private const string AppExeName = "OkPlayer.exe";
    // Where Assign() stashes the extension's prior per-user default ProgID, so Unassign() can put it back
    // instead of orphaning the type. Lives under the HKCU\Software\Classes\<ext> key it backs up.
    private const string PrevProgIdValue = "OkPlayerPrevProgId";

    private readonly string _exePath = Environment.ProcessPath ?? string.Empty;

    [DllImport("shell32.dll")]
    private static extern void SHChangeNotify(int eventId, uint flags, IntPtr item1, IntPtr item2);
    private const int SHCNE_ASSOCCHANGED = 0x08000000;

    /// <summary>False if we can't resolve our own exe path (then registration is unavailable).</summary>
    public bool CanRegister => !string.IsNullOrEmpty(_exePath);

    private bool _registered; // EnsureRegistered did its (idempotent) work this session — skip the redundant rewrite

    /// <summary>Register the ProgID, the Applications entry, and the Default-Apps capabilities. Idempotent, and
    /// cheap to call repeatedly (e.g. once per extension in a bulk assign) — the actual registry writes run only
    /// the first time per session.</summary>
    public void EnsureRegistered()
    {
        if (!CanRegister || _registered)
            return;
        string cmd = $"\"{_exePath}\" \"%1\"";
        string icon = $"\"{_exePath}\",0";

        using (var prog = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{ProgId}"))
        {
            prog.SetValue("", "OK Player media file");
            prog.SetValue("FriendlyTypeName", "OK Player media file");
            using (var di = prog.CreateSubKey("DefaultIcon")) di.SetValue("", icon);
            using (var oc = prog.CreateSubKey(@"shell\open\command")) oc.SetValue("", cmd);
        }
        using (var app = Registry.CurrentUser.CreateSubKey($@"Software\Classes\Applications\{AppExeName}"))
        {
            app.SetValue("FriendlyAppName", "OK Player");
            using (var di = app.CreateSubKey("DefaultIcon")) di.SetValue("", icon);
            using (var oc = app.CreateSubKey(@"shell\open\command")) oc.SetValue("", cmd);
        }
        using (var cap = Registry.CurrentUser.CreateSubKey(CapabilitiesKey))
        {
            cap.SetValue("ApplicationName", "OK Player");
            cap.SetValue("ApplicationDescription", "An elegant media player for Windows.");
            cap.SetValue("ApplicationIcon", icon); // so OK Player shows with its icon in Windows' Default Apps
        }
        using (var reg = Registry.CurrentUser.CreateSubKey(@"Software\RegisteredApplications"))
            reg.SetValue(AppRegName, CapabilitiesKey);
        _registered = true;
    }

    /// <summary>If the ProgID's open-command no longer points at the exe now running — e.g. the app was updated
    /// to a new install path (the Inno→Velopack move, or a future versioned layout) — rewrite the registration so
    /// a double-click / "Open with" launches THIS build instead of a removed one. Returns true if it rewrote
    /// anything. Deliberately a no-op when nothing is registered yet (don't create associations the user never
    /// asked for) and when the path already matches. Call on startup; wrap in try/catch (registry can throw).</summary>
    public bool RefreshCommandIfStale()
    {
        if (!CanRegister)
            return false;
        string want = $"\"{_exePath}\" \"%1\"";
        using (var oc = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{ProgId}\shell\open\command"))
        {
            if (oc?.GetValue("") is not string have)
                return false; // never registered → nothing to refresh; leave the user's associations untouched
            if (string.Equals(have, want, StringComparison.OrdinalIgnoreCase))
                return false; // already points at the current exe
        }
        EnsureRegistered(); // a fresh instance: _registered is false, so this rewrites the command to _exePath
        NotifyShell();
        return true;
    }

    /// <summary>True when OK Player is registered as a handler for this extension (the candidate registration the
    /// checkbox represents). Being registered is NOT the same as being the OS default: on Win11 a double-click
    /// only goes straight to us once the user confirms in Windows (which writes the hash-protected UserChoice) —
    /// see <see cref="HasForeignUserChoice"/> and <see cref="OpenWindowsDefaultApps"/>.</summary>
    public bool IsAssigned(string ext)
    {
        ext = Norm(ext);
        using var k = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{ext}\OpenWithProgids");
        return k?.GetValue(ProgId) is not null;
    }

    /// <summary>True when Windows has a hash-protected UserChoice for this extension pinned to a DIFFERENT app.
    /// In that state our default pointer is overridden, so assigning can't make a double-click open OK Player —
    /// only the user can switch it (Settings → Default apps). Lets the UI say so instead of silently no-op'ing.</summary>
    public bool HasForeignUserChoice(string ext)
    {
        ext = Norm(ext);
        using var uc = Registry.CurrentUser.OpenSubKey(
            $@"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice");
        return uc?.GetValue("ProgId") is string p && p.Length > 0 && !IsOurProgId(p);
    }

    // Windows records OUR default under either ProgID form: the ProgID we register, or the
    // "Applications\OkPlayer.exe" alias it writes when the user picks us from the Open-with chooser. Treat both
    // as ours so a confirmed OK Player default isn't mistaken for a foreign app.
    private static bool IsOurProgId(string? progId) =>
        string.Equals(progId, ProgId, StringComparison.OrdinalIgnoreCase)
        || string.Equals(progId, $@"Applications\{AppExeName}", StringComparison.OrdinalIgnoreCase);

    public void Assign(string ext)
    {
        EnsureRegistered();
        ext = Norm(ext);
        // Candidate registration: surfaces OK Player in "Open with" and in Windows' Default Apps list.
        using (var owp = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{ext}\OpenWithProgids"))
            owp.SetValue(ProgId, Array.Empty<byte>(), RegistryValueKind.None);
        using (var fa = Registry.CurrentUser.CreateSubKey($@"{CapabilitiesKey}\FileAssociations"))
            fa.SetValue(ext, ProgId);
        using (var st = Registry.CurrentUser.CreateSubKey($@"Software\Classes\Applications\{AppExeName}\SupportedTypes"))
            st.SetValue(ext, string.Empty);
        // Best-effort effective default: the legacy per-user ProgID pointer in HKCU\Software\Classes\<ext> wins
        // the HKCR merge over the HKLM default and gives the file OK Player's icon. On a pristine extension (no
        // prior handler) it can also make a double-click open us; but where Windows has any UserChoice or an
        // "Open with" history, it shows its picker until the user confirms once (verified empirically — the
        // pointer alone is not enough on Win11). It forges no UserChoice hash, so it needs no admin and trips no
        // AV/SmartScreen/UCPD guard. Back up any prior ProgID so Unassign can restore the user's handler.
        using (var cls = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{ext}"))
        {
            // Back up the real prior handler — but not our own ProgID/alias, or a re-assign would later "restore"
            // OK Player instead of the user's actual pre-OK-Player default.
            if (cls.GetValue("") is string prev && prev.Length > 0 && !IsOurProgId(prev))
                cls.SetValue(PrevProgIdValue, prev);
            cls.SetValue("", ProgId);
        }
    }

    public void Unassign(string ext)
    {
        ext = Norm(ext);
        using (var owp = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{ext}\OpenWithProgids", writable: true))
            owp?.DeleteValue(ProgId, throwOnMissingValue: false);
        using (var fa = Registry.CurrentUser.OpenSubKey($@"{CapabilitiesKey}\FileAssociations", writable: true))
            fa?.DeleteValue(ext, throwOnMissingValue: false);
        using (var st = Registry.CurrentUser.OpenSubKey($@"Software\Classes\Applications\{AppExeName}\SupportedTypes", writable: true))
            st?.DeleteValue(ext, throwOnMissingValue: false);
        // Roll back the default pointer only while it's still ours: restore the backed-up prior ProgID, or clear
        // it if there was none — never clobber a default the user re-pointed elsewhere since we set it.
        using (var cls = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{ext}", writable: true))
        {
            if (cls is not null)
            {
                // Roll the default back to the backup only while it's still ours — never clobber a default the
                // user re-pointed elsewhere since we set it.
                if (IsOurProgId(cls.GetValue("") as string))
                {
                    if (cls.GetValue(PrevProgIdValue) is string prev && prev.Length > 0)
                        cls.SetValue("", prev);
                    else
                        cls.DeleteValue("", throwOnMissingValue: false);
                }
                // Drop our backup either way: it's only meaningful while we own the default, and a stale copy
                // could otherwise restore an outdated ProgID on a later assign/unassign cycle.
                cls.DeleteValue(PrevProgIdValue, throwOnMissingValue: false);
            }
        }
    }

    /// <summary>Tell the shell that associations changed so Explorer / "Open with" refresh.</summary>
    public void NotifyShell() => SHChangeNotify(SHCNE_ASSOCCHANGED, 0, IntPtr.Zero, IntPtr.Zero);

    /// <summary>Open Windows' Default Apps page (deep-linked to OK Player) so the user can confirm defaults —
    /// the one step an app can't do silently on Windows 10/11.</summary>
    public static void OpenWindowsDefaultApps()
    {
        try
        {
            var psi = new System.Diagnostics.ProcessStartInfo("ms-settings:defaultapps?registeredAppUser=OK%20Player")
            { UseShellExecute = true };
            System.Diagnostics.Process.Start(psi);
        }
        catch { /* best effort */ }
    }

    private static string Norm(string ext)
        => ext.StartsWith('.') ? ext.ToLowerInvariant() : "." + ext.ToLowerInvariant();
}
