using System;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace OkPlayer.App.Services;

/// <summary>
/// Per-user (HKCU) file-association management for the unpackaged app. Registers a ProgID + an
/// "Applications\OkPlayer.exe" entry + Default-Apps capabilities, and assigns/unassigns OK Player as a
/// candidate handler for individual extensions. Windows 10/11 hash-protect the *default* handler
/// (UserChoice), so an app can make itself a candidate (it then shows in "Open with" and in Windows'
/// Default Apps) but the user must confirm "default" in Windows — see <see cref="OpenWindowsDefaultApps"/>.
/// No admin rights are needed.
/// </summary>
public sealed class FileAssociationService
{
    private const string ProgId = "OkPlayer.MediaFile";
    private const string AppRegName = "OK Player";
    private const string CapabilitiesKey = @"Software\OkPlayer\Capabilities";
    private const string AppExeName = "OkPlayer.exe";

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

    public bool IsAssigned(string ext)
    {
        ext = Norm(ext);
        using var k = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{ext}\OpenWithProgids");
        return k?.GetValue(ProgId) is not null;
    }

    public void Assign(string ext)
    {
        EnsureRegistered();
        ext = Norm(ext);
        using (var owp = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{ext}\OpenWithProgids"))
            owp.SetValue(ProgId, Array.Empty<byte>(), RegistryValueKind.None);
        using (var fa = Registry.CurrentUser.CreateSubKey($@"{CapabilitiesKey}\FileAssociations"))
            fa.SetValue(ext, ProgId);
        using (var st = Registry.CurrentUser.CreateSubKey($@"Software\Classes\Applications\{AppExeName}\SupportedTypes"))
            st.SetValue(ext, string.Empty);
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
