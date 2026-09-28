// The Rust core as a static library (adapters/build-native.sh puts it in lib/<Platform>/).
using System.IO;
using UnrealBuildTool;

public class SimcraftLib : ModuleRules
{
    public SimcraftLib(ReadOnlyTargetRules Target) : base(Target)
    {
        Type = ModuleType.External;
        PublicSystemIncludePaths.Add(Path.Combine(ModuleDirectory, "include"));
        string Lib = Path.Combine(ModuleDirectory, "lib", Target.Platform.ToString());

        if (Target.Platform == UnrealTargetPlatform.Win64)
        {
            PublicAdditionalLibraries.Add(Path.Combine(Lib, "simcraft.lib"));
            PublicSystemLibraries.AddRange(new[] { "ws2_32.lib", "userenv.lib", "ntdll.lib", "bcrypt.lib" });
        }
        else if (Target.Platform == UnrealTargetPlatform.Mac)
        {
            PublicAdditionalLibraries.Add(Path.Combine(Lib, "libsimcraft.a"));
            PublicSystemLibraries.Add("iconv");
        }
        else if (Target.Platform == UnrealTargetPlatform.Linux)
        {
            PublicAdditionalLibraries.Add(Path.Combine(Lib, "libsimcraft.a"));
        }
    }
}
