// P/Invoke bindings for include/simcraft.h (ABI 1). No UnityEngine: testable with plain .NET.
using System;
using System.Runtime.InteropServices;

namespace Simcraft
{
    /// <summary>One entity for one frame. Layout matches <c>SimcraftEntity</c> (32 bytes).</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Entity
    {
        public ulong Id;
        public long X;
        public long Y;
        /// <summary>Kind index; <see cref="Simulation.KindName"/> gives the name.</summary>
        public uint Kind;
        /// <summary>Unicode code point of the current state's glyph (the designer's state → look map).</summary>
        public uint Glyph;
    }

    internal static class Native
    {
#if (UNITY_IOS || UNITY_WEBGL) && !UNITY_EDITOR
        const string Lib = "__Internal"; // statically linked
#else
        const string Lib = "simcraft";   // libsimcraft.dylib / libsimcraft.so / simcraft.dll
#endif
        public const uint AbiVersion = 1;

        [DllImport(Lib)] public static extern uint simcraft_abi_version();
        [DllImport(Lib)] public static extern IntPtr simcraft_new(IntPtr gameRon, IntPtr engineToml, out IntPtr error);
        [DllImport(Lib)] public static extern void simcraft_free(IntPtr sim);
        [DllImport(Lib)] public static extern void simcraft_string_free(IntPtr s);
        [DllImport(Lib)] public static extern IntPtr simcraft_request(IntPtr sim, IntPtr requestJson);
        [DllImport(Lib)] public static extern long simcraft_step(IntPtr sim, uint n);
        [DllImport(Lib)] public static extern UIntPtr simcraft_entities(IntPtr sim, [Out] Entity[] output, UIntPtr cap);
        [DllImport(Lib)] public static extern IntPtr simcraft_kind_name(IntPtr sim, uint kind);
        [DllImport(Lib)] public static extern IntPtr simcraft_drain(IntPtr sim);

        /// <summary>UTF-8 copy for a call; free with <see cref="Marshal.FreeCoTaskMem"/>.</summary>
        public static IntPtr Utf8(string s) => Marshal.StringToCoTaskMemUTF8(s);

        /// <summary>Takes ownership of a string returned by the library.</summary>
        public static string Take(IntPtr p)
        {
            if (p == IntPtr.Zero) return null;
            try { return Marshal.PtrToStringUTF8(p); }
            finally { simcraft_string_free(p); }
        }
    }
}
