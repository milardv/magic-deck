// Adapter for Forge 2.0.14. Rules and AI are provided entirely by Forge.
// Run with a JDK 17+ source launcher and the complete Forge distribution on the classpath.
import forge.gui.GuiBase;
import forge.gui.interfaces.IGuiBase;
import forge.model.FModel;
import forge.deck.Deck;
import forge.item.PaperCard;
import forge.game.Game;
import forge.game.GameRules;
import forge.game.GameType;
import forge.game.Match;
import forge.game.player.RegisteredPlayer;
import forge.player.GamePlayerUtil;
import forge.util.BuildInfo;
import forge.util.MyRandom;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;

public class MagicDeckSimulation {
    // Forge's desktop facade initializes a physical screen even in CLI mode.
    // This facade supplies only non-interactive services; gameplay decisions remain AI-owned.
    private static IGuiBase headlessGui() {
        return (IGuiBase) java.lang.reflect.Proxy.newProxyInstance(
            IGuiBase.class.getClassLoader(), new Class<?>[]{IGuiBase.class}, (proxy, method, args) -> {
                switch (method.getName()) {
                    case "getAssetsDir": return "";
                    case "getCurrentVersion": return BuildInfo.getVersionString();
                    case "isRunningOnDesktop": case "isGuiThread": return true;
                    case "isLibgdxPort": case "hasNetGame": case "isSupportedAudioFormat": return false;
                    case "getAvatarCount": case "getSleevesCount": return 1;
                    case "getScreenScale": return 1.0f;
                    case "encodeSymbols": return args[0];
                    case "invokeInEdtNow": case "invokeInEdtLater": case "invokeInEdtAndWait":
                        ((Runnable)args[0]).run(); return null;
                    case "runBackgroundTask": ((Runnable)args[1]).run(); return null;
                    case "clearImageCache": case "preventSystemSleep": case "startAltSoundSystem": return null;
                    case "getSkinIcon": case "getUnskinnedIcon": case "getCardArt": case "createLayeredImage":
                    case "createAudioClip": case "createAudioMusic": return null;
                    default: throw new UnsupportedOperationException("Service graphique demandé : " + method.getName());
                }
            });
    }
    private static String quote(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : text.toCharArray()) {
            if (c == '\\' || c == '"') out.append('\\').append(c);
            else if (c < 32) out.append(String.format("\\u%04x", (int)c));
            else out.append(c);
        }
        return out.append('"').toString();
    }
    private static void emit(Map<String, ?> event) {
        StringJoiner json = new StringJoiner(",", "{", "}");
        event.forEach((key, value) -> json.add(quote(key) + ":" +
            (value instanceof Number ? value.toString() : quote(value.toString()))));
        System.out.println("MAGIC_DECK " + json);
        System.out.flush();
    }

    private static Deck load(Path path, String name) throws Exception {
        Deck deck = new Deck(name);
        List<String> missing = new ArrayList<>();
        boolean main = false;
        for (String line : Files.readAllLines(path)) {
            if (line.equals("[Main]")) { main = true; continue; }
            if (!main || line.isBlank()) continue;
            int space = line.indexOf(' ');
            int quantity = Integer.parseInt(line.substring(0, space));
            String cardName = line.substring(space + 1);
            PaperCard card = FModel.getMagicDb().getCommonCards().getCard(cardName);
            if (card == null || !card.getName().equalsIgnoreCase(cardName)) missing.add(cardName);
            else deck.getMain().add(card, quantity);
        }
        if (!missing.isEmpty()) throw new IllegalArgumentException(
            "Cartes absentes de Forge dans " + name + " : " + String.join(", ", missing));
        return deck;
    }

    public static void main(String[] args) {
        try {
            // Avoid Forge desktop bootstrap, telemetry and GUI startup. A private user.home
            // (and APPDATA on Windows) is supplied by Rust for every campaign.
            GuiBase.setInterface(headlessGui());
            FModel.initialize(null, null);
            Deck challenger = load(Path.of(args[0]), "Challenger");
            int separator = -1;
            for (int i = 1; i < args.length; i++) if (args[i].equals("--")) { separator = i; break; }
            if (separator < 2 || separator + 3 >= args.length) throw new IllegalArgumentException("Arguments de simulation incomplets");
            List<Deck> opponents = new ArrayList<>();
            for (int i = 1; i < separator; i++) opponents.add(load(Path.of(args[i]), "Opponent " + i));
            int count = Integer.parseInt(args[separator + 1]);
            long seed = Long.parseLong(args[separator + 2]);
            int seconds = Integer.parseInt(args[separator + 3]);
            emit(Map.of("kind", "ready", "version", BuildInfo.getVersionString()));
            // Forge is not documented as thread-safe, so games stay sequential, but the
            // worker thread is reused for the whole campaign to avoid per-game churn.
            ExecutorService gameWorker = Executors.newSingleThreadExecutor(r -> {
                Thread t = new Thread(r, "magic-deck-game"); t.setDaemon(true); return t;
            });
            for (int opponentIndex = 0; opponentIndex < opponents.size(); opponentIndex++) {
                Deck opponent = opponents.get(opponentIndex);
                for (int gameIndex = 0; gameIndex < count; gameIndex++) {
                int i = opponentIndex * count + gameIndex;
                // Reset the entire match for every BO1; swap seats, not the actual die roll.
                MyRandom.setRandom(new Random(seed + i));
                RegisteredPlayer a = new RegisteredPlayer(challenger).setPlayer(
                    GamePlayerUtil.createAiPlayer("Challenger", 0, ""));
            RegisteredPlayer b = new RegisteredPlayer(opponent).setPlayer(
                    GamePlayerUtil.createAiPlayer("Opponent", 1, ""));
                GameRules rules = new GameRules(GameType.Constructed);
                rules.setGamesPerMatch(1);
                Match match = new Match(rules, i % 2 == 0 ? List.of(a, b) : List.of(b, a), "Magic Deck");
                Game game = match.createGame();
                // Available in newer Forge versions; 2.0.14 runs headless without it.
                try { game.getClass().getMethod("setNoGUIUser").invoke(game); }
                catch (NoSuchMethodException ignored) { }
                long start = System.nanoTime();
                Future<?> future = gameWorker.submit(() -> match.startGame(game));
                try {
                    future.get(seconds, TimeUnit.SECONDS);
                    if (!game.isGameOver() || game.getOutcome() == null)
                        throw new IllegalStateException("Forge n'a pas terminé la partie");
                    String result = game.getOutcome().isDraw() ? "draw" :
                        game.getOutcome().getWinningLobbyPlayer().getName().equals("Challenger") ? "win" : "loss";
                    emit(Map.of("kind", "game", "index", i + 1, "opponentIndex", opponentIndex, "outcome", result,
                        "durationMs", (System.nanoTime() - start) / 1_000_000,
                        "turns", game.getPhaseHandler().getTurn(), "seat", i % 2 + 1));
                } catch (TimeoutException error) {
                    emit(Map.of("kind", "game", "index", i + 1, "opponentIndex", opponentIndex, "outcome", "timeout",
                        "durationMs", (System.nanoTime() - start) / 1_000_000, "turns", 0, "seat", i % 2 + 1));
                    throw new IllegalStateException("Délai dépassé : série arrêtée, ce résultat n'est pas une égalité.");
                } finally { future.cancel(true); }
            }
            }
            gameWorker.shutdownNow();
            emit(Map.of("kind", "done"));
            System.exit(0); // Forge owns additional executor threads.
        } catch (Throwable error) {
            error.printStackTrace(System.err);
            Throwable cause = error.getCause() == null ? error : error.getCause();
            emit(Map.of("kind", "error", "message", cause.toString()));
            System.exit(2);
        }
    }
}
