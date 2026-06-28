using System;
using System.Threading;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using OkPlayer.App.Services;

namespace OkPlayer.App;

/// <summary>The process entry point. Replaces the SDK's generated <c>Main</c> (see
/// <c>DISABLE_XAML_GENERATED_MAIN</c> in the csproj) so we can make OK Player single-instance: a second launch
/// — e.g. double-clicking another file in Explorer, or a file association — forwards its activation to the
/// already-running instance instead of spawning a second window. Without this, every double-click opened a new
/// process.</summary>
public static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        // Diagnostics first — before anything can fault — so a launch-time crash/hang is on disk. Best-effort.
        Log.Init();
        Log.InstallGlobalHandlers();
        Log.Step("Program.Main: start");

        WinRT.ComWrappersSupport.InitializeComWrappers();

        // Failsafe: any failure inside the single-instance path falls through to a normal launch, so a quirk in
        // the AppLifecycle plumbing can never stop the app from starting (worst case = the old multi-instance
        // behaviour, never a non-starting app).
        try
        {
            if (RedirectToPrimaryInstance())
            {
                Log.Step("single-instance: redirected to primary; this process exits");
                return 0; // we handed this launch to the running instance; this process exits
            }
        }
        catch (Exception ex) { Log.Exception("RedirectToPrimaryInstance", ex); /* fall through to a normal launch */ }

        Log.Step("Application.Start");
        Application.Start(p =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        Log.Step("Application.Start returned (process shutting down)");
        return 0;
    }

    /// <summary>Claim (or find) the single-instance key. If another instance already owns it, forward this
    /// launch's activation to it (so our file opens there) and report <c>true</c> so we exit. Otherwise we are
    /// the primary: wire the handler that later redirects land on, and report <c>false</c> so we start normally.</summary>
    private static bool RedirectToPrimaryInstance()
    {
        AppInstance primary = AppInstance.FindOrRegisterForKey("OkPlayer-single-instance");
        if (primary.IsCurrent)
        {
            // The Activated event fires on a background thread; App marshals to the UI thread before touching
            // the window. Application.Current is null now but set by the time a redirect actually arrives.
            primary.Activated += (_, e) => (Application.Current as App)?.OnRedirectedActivation(e);
            return false;
        }

        AppActivationArguments activation = AppInstance.GetCurrent().GetActivatedEventArgs();
        Log.Step("single-instance: not primary — redirecting activation (blocking)");
        primary.RedirectActivationToAsync(activation).AsTask().GetAwaiter().GetResult();
        Log.Step("single-instance: redirect completed");
        return true;
    }
}
