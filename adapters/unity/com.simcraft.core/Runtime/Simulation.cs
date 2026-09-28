// One running game. No UnityEngine: SimcraftWorld is the Unity face of this class.
using System;
using System.Runtime.InteropServices;

namespace Simcraft
{
    public sealed class SimcraftException : Exception
    {
        /// <summary>The JSON the core returned, e.g. {"ok":false,"stage":"validate","errors":[...]}.</summary>
        public string Json { get; }
        public SimcraftException(string json) : base(json) => Json = json;
    }

    public sealed class Simulation : IDisposable
    {
        IntPtr _sim;

        Simulation(IntPtr sim) => _sim = sim;

        /// <summary>Loads a game from the text of game.ron and engine.toml.</summary>
        /// <exception cref="SimcraftException">Load or validation failed; <c>Json</c> lists every error.</exception>
        public static Simulation Load(string gameRon, string engineToml)
        {
            var abi = Native.simcraft_abi_version();
            if (abi != Native.AbiVersion)
                throw new SimcraftException($"{{\"ok\":false,\"error\":\"native library ABI {abi}, package expects {Native.AbiVersion}\"}}");
            IntPtr game = Native.Utf8(gameRon), panel = Native.Utf8(engineToml);
            try
            {
                var sim = Native.simcraft_new(game, panel, out var error);
                if (sim == IntPtr.Zero) throw new SimcraftException(Native.Take(error));
                return new Simulation(sim);
            }
            finally
            {
                Marshal.FreeCoTaskMem(game);
                Marshal.FreeCoTaskMem(panel);
            }
        }

        IntPtr Handle => _sim != IntPtr.Zero ? _sim : throw new ObjectDisposedException(nameof(Simulation));

        /// <summary>Any agent-protocol request (info, observe, act, step, hash, snapshot, restore) → JSON reply.</summary>
        public string Request(string json)
        {
            var req = Native.Utf8(json);
            try { return Native.Take(Native.simcraft_request(Handle, req)); }
            finally { Marshal.FreeCoTaskMem(req); }
        }

        /// <summary>Advances up to <paramref name="n"/> ticks; returns the tick.</summary>
        public long Step(uint n = 1) => Native.simcraft_step(Handle, n);

        /// <summary>Copies every entity into <paramref name="buffer"/> (grown when needed); returns the count.</summary>
        public int Entities(ref Entity[] buffer)
        {
            buffer ??= new Entity[64];
            var total = (int)Native.simcraft_entities(Handle, buffer, (UIntPtr)buffer.Length);
            if (total > buffer.Length)
            {
                buffer = new Entity[total * 2];
                total = (int)Native.simcraft_entities(Handle, buffer, (UIntPtr)buffer.Length);
            }
            return total;
        }

        public string KindName(uint kind) => Marshal.PtrToStringUTF8(Native.simcraft_kind_name(Handle, kind));

        /// <summary>Every bus message since the last call, as a JSON array.</summary>
        public string Drain() => Native.Take(Native.simcraft_drain(Handle));

        /// <summary>The whole game at this tick. Keep the string as the save file.</summary>
        public string Save() => Request("{\"cmd\":\"snapshot\"}");

        /// <summary>Back to a save made by <see cref="Save"/>; the future is bit-identical.</summary>
        /// <exception cref="SimcraftException">The save is from another game or version.</exception>
        public void Load(string save)
        {
            var body = save.Trim();
            if (!body.StartsWith("{")) throw new SimcraftException("{\"ok\":false,\"error\":\"a save is a JSON object\"}");
            var reply = Request("{\"cmd\":\"restore\"," + body.Substring(1));
            if (!reply.Contains("\"ok\":true")) throw new SimcraftException(reply);
        }

        public void Dispose()
        {
            if (_sim == IntPtr.Zero) return;
            Native.simcraft_free(_sim);
            _sim = IntPtr.Zero;
        }
    }
}
