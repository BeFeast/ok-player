import OKPlayerShell

// The bundle's executable. Everything the shell does lives in the OKPlayerShell
// library, where the test target can reach it.
@main
@MainActor
enum OKPlayerMain {
    static func main() {
        PlayerLauncher.run()
    }
}
