//! Santa gift quotes (ui 3.9). Own short lines, one list per language.
//! The lists are independent of each other (not translations).

/// One quote. `by` is the optional author line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quote {
    pub text: &'static str,
    pub by: Option<&'static str>,
}

macro_rules! q {
    ($text:expr) => {
        Quote { text: $text, by: None }
    };
}

pub(crate) static EN: &[Quote] = &[
    q!("Be as kind as a first snowfall: quiet, and for everyone."),
    q!("Small steps still leave footprints in the snow."),
    q!("Every big snowman began as one small ball."),
    q!("Warm hands, warm heart, warm cocoa."),
    q!("Not every day needs to be bright. Some are just soft."),
    q!("The best part of winter is the people you share it with."),
    q!("Rest is part of the work."),
    q!("Even the longest night ends with a sunrise."),
    q!("You are doing better than you think."),
    q!("Leave a little room for joy today."),
    q!("Snowflakes are proof that being different is beautiful."),
    q!("Take the slow road. The view is better."),
    Quote {
        text: "On the quietest mornings the whole world is wrapped in white, and every small sound feels like a gift.",
        by: Some("An old winter saying"),
    },
];

pub(crate) static JA: &[Quote] = &[
    q!("初雪のようにやさしく。静かに、みんなのために。"),
    q!("小さな一歩も、雪の上にはちゃんと足跡が残ります。"),
    q!("大きな雪だるまも、はじまりは小さな雪玉でした。"),
    q!("あたたかい手、あたたかい心、あたたかいココア。"),
    q!("毎日が輝かなくてもいい。やわらかな日があってもいい。"),
    q!("休むことも、仕事のうちです。"),
    q!("どんなに長い夜も、朝になれば明けます。"),
    q!("あなたは、思っているよりずっとがんばっています。"),
    q!("今日は少しだけ、楽しむ余白を残しましょう。"),
    q!("雪の結晶は、違っていることの美しさの証です。"),
    Quote {
        text: "いちばん静かな朝には、世界じゅうが白につつまれて、どんなに小さな音もささやかな贈り物のように聞こえます。",
        by: Some("冬の言い伝え"),
    },
];

pub(crate) static ES: &[Quote] = &[
    q!("Sé amable como la primera nevada: en silencio y para todos."),
    q!("Hasta los pasos pequeños dejan huella en la nieve."),
    q!("Todo muñeco enorme empezó siendo una bola pequeña."),
    q!("Manos cálidas, corazón cálido, chocolate caliente."),
    q!("No todos los días tienen que brillar. Algunos solo son suaves."),
    q!("Descansar también es parte del trabajo."),
    q!("La noche más larga termina con un amanecer."),
    q!("Lo estás haciendo mejor de lo que crees."),
    q!("Deja hoy un huequito para la alegría."),
    q!("Los copos de nieve demuestran que ser distinto es hermoso."),
    Quote {
        text: "En las mañanas más tranquilas el mundo entero se envuelve en blanco, y hasta el sonido más pequeño parece un regalo.",
        by: Some("Dicho de invierno"),
    },
];

pub(crate) static DE: &[Quote] = &[
    q!("Sei freundlich wie der erste Schnee: leise und für alle da."),
    q!("Auch kleine Schritte hinterlassen Spuren im Schnee."),
    q!("Jeder große Schneemann begann als kleine Kugel."),
    q!("Warme Hände, warmes Herz, warmer Kakao."),
    q!("Nicht jeder Tag muss strahlen. Manche sind einfach weich."),
    q!("Ausruhen gehört zur Arbeit dazu."),
    q!("Auch die längste Nacht endet mit einem Sonnenaufgang."),
    q!("Du machst das besser, als du denkst."),
    q!("Lass heute ein bisschen Platz für Freude."),
    q!("Schneeflocken zeigen: Anderssein ist schön."),
    Quote {
        text: "An den stillsten Morgenden liegt die Welt in Weiß gehüllt, und jedes kleine Geräusch fühlt sich wie ein Geschenk an.",
        by: Some("Alte Winterweisheit"),
    },
];

pub(crate) static FR: &[Quote] = &[
    q!("Sois doux comme la première neige : discret et pour tous."),
    q!("Même les petits pas laissent des traces dans la neige."),
    q!("Tout grand bonhomme de neige a commencé par une petite boule."),
    q!("Mains chaudes, cœur chaud, chocolat chaud."),
    q!("Tous les jours n’ont pas besoin d’éclater. Certains sont simplement doux."),
    q!("Se reposer fait partie du travail."),
    q!("La nuit la plus longue finit par un lever de soleil."),
    q!("Tu t’en sors mieux que tu ne le penses."),
    q!("Garde aujourd’hui un peu de place pour la joie."),
    q!("Les flocons prouvent qu’être différent est beau."),
    Quote {
        text: "Les matins les plus calmes, le monde entier s’enveloppe de blanc, et le moindre bruit ressemble à un cadeau.",
        by: Some("Dicton d’hiver"),
    },
];

pub(crate) static KO: &[Quote] = &[
    q!("첫눈처럼 다정하게. 조용히, 모두를 위해."),
    q!("작은 발걸음도 눈 위에는 발자국을 남겨요."),
    q!("큰 눈사람도 처음엔 작은 눈덩이였어요."),
    q!("따뜻한 손, 따뜻한 마음, 따뜻한 코코아."),
    q!("매일이 반짝일 필요는 없어요. 어떤 날은 그냥 포근하면 돼요."),
    q!("쉬는 것도 일의 일부예요."),
    q!("아무리 긴 밤도 아침이 오면 끝나요."),
    q!("생각보다 훨씬 잘하고 있어요."),
    q!("오늘은 기쁨이 들어올 자리를 조금 남겨 두세요."),
    q!("눈송이는 다르다는 게 아름답다는 증거예요."),
    Quote {
        text: "가장 고요한 아침에는 온 세상이 하얗게 포근히 감싸이고, 아주 작은 소리마저 선물처럼 느껴져요.",
        by: Some("겨울의 옛말"),
    },
];

pub(crate) static RU: &[Quote] = &[
    q!("Будь добр, как первый снег: тихо и для всех."),
    q!("Даже маленькие шаги оставляют следы на снегу."),
    q!("Любой большой снеговик начинался с маленького кома."),
    q!("Тёплые руки, тёплое сердце, тёплое какао."),
    q!("Не каждый день должен сиять. Некоторые просто мягкие."),
    q!("Отдых — тоже часть работы."),
    q!("Даже самая долгая ночь заканчивается рассветом."),
    q!("У тебя получается лучше, чем ты думаешь."),
    q!("Оставь сегодня немного места для радости."),
    q!("Снежинки доказывают, что быть другим — красиво."),
    Quote {
        text: "Тихим утром весь мир укутан в белое, и даже самый тихий звук кажется маленьким подарком.",
        by: Some("Старая зимняя поговорка"),
    },
];

pub(crate) static TR: &[Quote] = &[
    q!("Kar yağarken acele etme; her tane kendi yerini bulur."),
    q!("Sıcak bir çay, iyi bir dost, biraz da kar. Bu kadarı yeter."),
    q!("Küçücük bir kartopu bile koca bir gülümseme başlatabilir."),
    q!("Bugün kendine biraz nazik davran."),
    q!("Her kış, içinde bir bahar saklar."),
    q!("Yorulduysan dinlen. Kar da bazen durur."),
    q!("Penceredeki buğuya bir kalp çiz."),
    q!("Sandığından çok daha iyi gidiyorsun."),
    q!("En soğuk günler, en sıcak sohbetleri getirir."),
    q!("İki kar tanesi bile aynı değil. Sen de eşsizsin."),
    Quote {
        text: "Kar sessizce yağdığında şehir bir an durur ve insan kendi kalbinin sesini duyar.",
        by: Some("Eski bir kış sözü"),
    },
];

pub(crate) static KK: &[Quote] = &[
    q!("Қыс ұзақ болса да, наурыз бәрібір келеді."),
    q!("Ыстық шай мен жылы сөз — қыстың ең жақсы емі."),
    q!("Үлкен аққала да бір уыс қардан басталады."),
    q!("Бүгін өзіңе сәл мейірімді бол."),
    q!("Ақ қар астында жер тынығып жатыр. Сен де тынық."),
    q!("Терезедегі буға күн суретін сал."),
    q!("Сен ойлағаныңнан әлдеқайда жақсы істеп жүрсің."),
    q!("Достың жылуы аязды да жеңеді."),
    q!("Әр қар ұшқыны өзгеше. Сен де солайсың."),
    q!("Асықпа: қар да баяу жауып, әлемді әдемілейді."),
    Quote {
        text: "Тып-тыныш таңда аппақ далада әр қадамның сықыры әуен сияқты естіледі.", by: Some("Қысқы нақыл")
    },
];
