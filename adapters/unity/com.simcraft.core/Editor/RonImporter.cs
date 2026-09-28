// game.ron and engine.toml become TextAssets, so they can be dragged onto SimcraftWorld.
using System.IO;
using UnityEditor.AssetImporters;
using UnityEngine;

namespace Simcraft.Editor
{
    [ScriptedImporter(1, new[] { "ron", "toml" })]
    public class RonImporter : ScriptedImporter
    {
        public override void OnImportAsset(AssetImportContext ctx)
        {
            var text = new TextAsset(File.ReadAllText(ctx.assetPath));
            ctx.AddObjectToAsset("text", text);
            ctx.SetMainObject(text);
        }
    }
}
