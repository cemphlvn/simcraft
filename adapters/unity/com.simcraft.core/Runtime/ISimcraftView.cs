namespace Simcraft
{
    /// <summary>
    /// Put on a kind's prefab to react to the simulation. The core decides; the view only shows.
    /// </summary>
    public interface ISimcraftView
    {
        /// <summary>Called once when the entity appears.</summary>
        void OnSimcraftSpawn(ulong id, string kind);

        /// <summary>Called when the state's glyph changes (the designer maps states to glyphs in game.ron).</summary>
        void OnSimcraftState(char glyph);
    }
}
