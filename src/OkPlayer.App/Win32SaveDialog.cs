using System;
using System.Runtime.InteropServices;

namespace OkPlayer.App;

/// <summary>A native Save-As dialog via the Win32 Common Item Dialog (IFileSaveDialog). The WinRT
/// <c>FileSavePicker</c> is unreliable in this unpackaged WinUI 3 app — it throws E_FAIL or hangs depending on
/// the activation/window state — so we drive the shell dialog directly. It parents to our HWND and returns the
/// chosen path synchronously (Show runs its own modal message loop, so the app stays responsive).</summary>
internal static class Win32SaveDialog
{
    /// <summary>Show a Save-As dialog owned by <paramref name="ownerHwnd"/>; returns the chosen full path, or
    /// null if the user cancelled or anything failed. Must be called on the window's UI (STA) thread.</summary>
    public static string? PickSavePath(IntPtr ownerHwnd, string suggestedName, string typeLabel, string ext)
    {
        IFileSaveDialog? dlg = null;
        try
        {
            dlg = (IFileSaveDialog)new FileSaveDialogRcw();
            dlg.SetFileTypes(1, new[] { new COMDLG_FILTERSPEC { pszName = typeLabel, pszSpec = "*." + ext } });
            dlg.SetFileTypeIndex(1);
            dlg.SetDefaultExtension(ext);
            dlg.SetFileName(suggestedName);
            dlg.GetOptions(out uint opts);
            dlg.SetOptions(opts | FOS_OVERWRITEPROMPT);

            int hr = dlg.Show(ownerHwnd);
            if (hr == ERROR_CANCELLED)
                return null;
            Marshal.ThrowExceptionForHR(hr);

            dlg.GetResult(out IShellItem item);
            try
            {
                item.GetDisplayName(SIGDN_FILESYSPATH, out IntPtr pszPath);
                try { return Marshal.PtrToStringUni(pszPath); }
                finally { Marshal.FreeCoTaskMem(pszPath); }
            }
            finally { Marshal.ReleaseComObject(item); }
        }
        catch { return null; }
        finally { if (dlg is not null) Marshal.ReleaseComObject(dlg); }
    }

    private const uint SIGDN_FILESYSPATH = 0x80058000;
    private const uint FOS_OVERWRITEPROMPT = 0x2;
    private const int ERROR_CANCELLED = unchecked((int)0x800704C7);

    [ComImport, Guid("C0B4E2F3-BA21-4773-8DBA-335EC946EB8B")]
    private class FileSaveDialogRcw { }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct COMDLG_FILTERSPEC
    {
        public string pszName;
        public string pszSpec;
    }

    // IFileSaveDialog : IFileDialog : IModalWindow — the full vtable in order; unused slots must still be
    // declared so the methods we call land on the right slot.
    [ComImport, Guid("84bccd23-5fde-4cdb-aea4-af64b83d78ab"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IFileSaveDialog
    {
        // IModalWindow
        [PreserveSig] int Show(IntPtr parent);
        // IFileDialog
        void SetFileTypes(uint cFileTypes, [MarshalAs(UnmanagedType.LPArray)] COMDLG_FILTERSPEC[] rgFilterSpec);
        void SetFileTypeIndex(uint iFileType);
        void GetFileTypeIndex(out uint piFileType);
        void Advise(IntPtr pfde, out uint pdwCookie);
        void Unadvise(uint dwCookie);
        void SetOptions(uint fos);
        void GetOptions(out uint pfos);
        void SetDefaultFolder(IShellItem psi);
        void SetFolder(IShellItem psi);
        void GetFolder(out IShellItem ppsi);
        void GetCurrentSelection(out IShellItem ppsi);
        void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string pszName);
        void GetFileName([MarshalAs(UnmanagedType.LPWStr)] out string pszName);
        void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string pszTitle);
        void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string pszText);
        void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
        void GetResult(out IShellItem ppsi);
        void AddPlace(IShellItem psi, int fdap);
        void SetDefaultExtension([MarshalAs(UnmanagedType.LPWStr)] string pszDefaultExtension);
        void Close([MarshalAs(UnmanagedType.Error)] int hr);
        void SetClientGuid(ref Guid guid);
        void ClearClientData();
        void SetFilter(IntPtr pFilter);
        // IFileSaveDialog
        void SetSaveAsItem(IShellItem psi);
        void SetProperties(IntPtr pStore);
        void SetCollectedProperties(IntPtr pList, int fAppendDefault);
        void GetProperties(out IntPtr ppStore);
        void ApplyProperties(IShellItem psi, IntPtr pStore, IntPtr hwnd, IntPtr pSink);
    }

    [ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellItem
    {
        void BindToHandler(IntPtr pbc, ref Guid bhid, ref Guid riid, out IntPtr ppv);
        void GetParent(out IShellItem ppsi);
        void GetDisplayName(uint sigdnName, out IntPtr ppszName);
        void GetAttributes(uint sfgaoMask, out uint psfgaoAttribs);
        void Compare(IShellItem psi, uint hint, out int piOrder);
    }
}
