using System.Runtime.InteropServices;
using Silk.NET.Core.Native;

namespace OkPlayer.Render.Interop;

/// <summary>Native interface behind WinUI's SwapChainPanel, used to bind a DXGI swap chain to it.</summary>
[ComImport]
[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
[Guid("63aad0b8-7c24-40ff-85a8-640d944cc325")]
public interface ISwapChainPanelNative
{
    [PreserveSig] HResult SetSwapChain([In] IntPtr swapChain);
    [PreserveSig] ulong Release();
}
