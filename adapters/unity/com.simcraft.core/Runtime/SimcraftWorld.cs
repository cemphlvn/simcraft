// Unity face of a Simulation: ticks at a fixed rate, keeps one GameObject per entity.
using System;
using System.Collections.Generic;
using UnityEngine;

namespace Simcraft
{
    [Serializable]
    public class KindView
    {
        public string kind;
        public GameObject prefab;
    }

    [AddComponentMenu("Simcraft/Simcraft World")]
    public class SimcraftWorld : MonoBehaviour
    {
        [Tooltip("The designer's file (games/<name>/game.ron).")]
        public TextAsset game;
        [Tooltip("The operator's panel (games/<name>/engine.toml).")]
        public TextAsset engine;

        [Min(0.01f)] public float ticksPerSecond = 10f;
        public bool playOnStart = true;
        [Tooltip("World units per grid cell. Grid x → X, grid y → -Z.")]
        public float cellSize = 1f;
        [Tooltip("Prefab per kind. A kind without one gets a coloured cube.")]
        public List<KindView> views = new List<KindView>();

        /// <summary>Every bus message since the last frame, as a JSON array (events, ticks, acts...).</summary>
        public event Action<string> BusMessages;

        public Simulation Sim { get; private set; }
        public bool Playing { get; set; }
        public long Tick { get; private set; }

        readonly Dictionary<ulong, (GameObject go, uint glyph)> _live = new Dictionary<ulong, (GameObject, uint)>();
        readonly HashSet<ulong> _seen = new HashSet<ulong>();
        readonly List<ulong> _gone = new List<ulong>();
        Entity[] _buffer;
        float _clock;

        void Awake()
        {
            if (game == null || engine == null)
            {
                Debug.LogError("SimcraftWorld: assign game (game.ron) and engine (engine.toml).", this);
                enabled = false;
                return;
            }
            try
            {
                Sim = Simulation.Load(game.text, engine.text);
            }
            catch (SimcraftException e)
            {
                Debug.LogError($"simcraft could not load {game.name}: {e.Json}", this);
                enabled = false;
                return;
            }
            Playing = playOnStart;
            Sync();
        }

        void Update()
        {
            if (!Playing) return;
            _clock += Time.deltaTime;
            var dt = 1f / ticksPerSecond;
            uint n = 0;
            while (_clock >= dt && n < 64) { _clock -= dt; n++; }
            if (n == 0) return;
            Tick = Sim.Step(n);
            Sync();
        }

        /// <summary>Advance by hand (e.g. turn-based games with playOnStart off).</summary>
        public void StepOnce(uint n = 1)
        {
            Tick = Sim.Step(n);
            Sync();
        }

        /// <summary>Save: the string is the whole game at this tick.</summary>
        public string Save() => Sim.Save();

        public void Load(string save)
        {
            Sim.Load(save);
            Sync();
        }

        void Sync()
        {
            var count = Sim.Entities(ref _buffer);
            _seen.Clear();
            for (var i = 0; i < count; i++)
            {
                var e = _buffer[i];
                _seen.Add(e.Id);
                if (!_live.TryGetValue(e.Id, out var view))
                {
                    var kind = Sim.KindName(e.Kind);
                    view = (Spawn(kind), uint.MaxValue);
                    view.go.name = $"{kind} {e.Id}";
                    foreach (var v in view.go.GetComponents<ISimcraftView>()) v.OnSimcraftSpawn(e.Id, kind);
                }
                view.go.transform.localPosition = new Vector3(e.X * cellSize, 0f, -e.Y * cellSize);
                if (view.glyph != e.Glyph)
                {
                    view.glyph = e.Glyph;
                    var glyph = char.ConvertFromUtf32((int)e.Glyph)[0];
                    foreach (var v in view.go.GetComponents<ISimcraftView>()) v.OnSimcraftState(glyph);
                }
                _live[e.Id] = view;
            }
            _gone.Clear();
            foreach (var id in _live.Keys) if (!_seen.Contains(id)) _gone.Add(id);
            foreach (var id in _gone)
            {
                Destroy(_live[id].go);
                _live.Remove(id);
            }
            var msgs = Sim.Drain();
            if (msgs != "[]") BusMessages?.Invoke(msgs);
        }

        GameObject Spawn(string kind)
        {
            var prefab = views.Find(v => v.kind == kind)?.prefab;
            if (prefab != null) return Instantiate(prefab, transform);
            var cube = GameObject.CreatePrimitive(PrimitiveType.Cube);
            cube.transform.SetParent(transform, false);
            cube.transform.localScale = Vector3.one * (cellSize * 0.8f);
            var hue = (Mathf.Abs(kind.GetHashCode()) % 360) / 360f;
            cube.GetComponent<Renderer>().material.color = Color.HSVToRGB(hue, 0.6f, 0.9f);
            return cube;
        }

        void OnDestroy()
        {
            Sim?.Dispose();
            Sim = null;
        }
    }
}
