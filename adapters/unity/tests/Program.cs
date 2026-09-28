// The C# wrapper against the real native library. Exit code 0 = all passed.
using System;
using System.IO;
using System.Runtime.InteropServices;
using Simcraft;

static class Program
{
    static int failed;

    static void Check(bool ok, string what)
    {
        Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what);
        if (!ok) failed++;
    }

    static int Main()
    {
        var root = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../.."));
        string Read(string game, string file) => File.ReadAllText(Path.Combine(root, "games", game, file));
        var ron = Read("wolf_sheep", "game.ron");
        var toml = Read("wolf_sheep", "engine.toml");

        Check(Marshal.SizeOf<Entity>() == 32, "Entity is 32 bytes, like SimcraftEntity");

        using (var sim = Simulation.Load(ron, toml))
        {
            Check(sim.Step(300) == 300, "Step(300) reaches tick 300");
            Check(sim.Request("{\"cmd\":\"hash\"}").Contains("ee9a66d10246d6f9"), "golden wolf/sheep hash, same as Rust and C");

            Entity[] buffer = null;
            var n = sim.Entities(ref buffer);
            int wolves = 0;
            for (var i = 0; i < n; i++)
                if (sim.KindName(buffer[i].Kind) == "wolf" && buffer[i].Glyph == 'W') wolves++;
            Check(n > 0 && wolves > 0, $"Entities: {n} entities, {wolves} wolves with glyph 'W'");

            var save = sim.Save();
            sim.Step(50);
            var later = sim.Request("{\"cmd\":\"hash\"}");
            sim.Load(save);
            sim.Step(50);
            Check(sim.Request("{\"cmd\":\"hash\"}") == later, "Save/Load: the future after Load is identical");

            Check(sim.Drain().Contains("\"t\":\"tick\""), "Drain returns bus messages");
            Check(sim.Drain() == "[]", "Drain empties");

            var fire = Simulation.Load(Read("forest_fire", "game.ron"), Read("forest_fire", "engine.toml"));
            try { fire.Load(save); Check(false, "a wolf/sheep save is refused by forest fire"); }
            catch (SimcraftException e) { Check(e.Json.Contains("source"), "a wolf/sheep save is refused by forest fire"); }
            fire.Dispose();
        }

        try { Simulation.Load(ron.Replace("me.hunger >= p.starve_at", "me.hungr >= p.starve_at"), toml); Check(false, "typo is reported"); }
        catch (SimcraftException e) { Check(e.Json.Contains("\"stage\":\"validate\""), "a typo in game.ron comes back as SimcraftException (validate)"); }

        var gone = Simulation.Load(ron, toml);
        gone.Dispose();
        gone.Dispose();
        try { gone.Step(); Check(false, "use after Dispose throws"); }
        catch (ObjectDisposedException) { Check(true, "use after Dispose throws; double Dispose is safe"); }

        Console.WriteLine(failed == 0 ? "all passed" : $"{failed} failed");
        return failed == 0 ? 0 : 1;
    }
}
