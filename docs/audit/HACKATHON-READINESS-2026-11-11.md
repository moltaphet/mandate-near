# بررسی آمادگی Mandate برای ارسال هکاتون

تاریخ بررسی: ۱۱ نوامبر ۲۰۲۶. هدف ارسال طبق درخواست فعلی: فردا، ۱۲ نوامبر، به وقت تهران.
نسخهٔ بررسی‌شده: `24d0434cd934027902dabc3a319c3aed89fec64f` روی شاخهٔ `feat/site-polish`.
این گزارش بررسی هدفمند آمادگی دمو، کد و شواهد است؛ ممیزی امنیتی مستقل و حرفه‌ای نیست.

**نتیجه: هستهٔ فنی و شواهد مهم testnet وجود دارند؛ ارسال نهایی هنوز مشروط به تکمیل مسیر دمو، هماهنگی مستندات و تأیید نسخهٔ منتشرشده است.** اگر پروژه در مسیر AI ارائه می‌شود، نبود معاملهٔ واقعی با تصمیم مدل مهم‌ترین شکاف ارائه است. نیاز به بازنویسی پروژه دیده نشد. توسعهٔ قابلیت‌های بزرگ جدید تا فردا اولویت ندارد.

نام هکاتون، track، فرم ارسال، ساعت رسمی پایان و شروط الزامی در این بررسی تأیید نشدند. برنامهٔ قدیمی `docs/superpowers/plans/testnet-checklist.md:263` از مهلت ۱۴ نوامبر صحبت می‌کند؛ این گزارش درخواست جدیدِ ارسال در ۱۲ نوامبر را مبنا می‌گیرد. فایل یا لینک ویدئو و pitch deck در فایل‌های tracked و صفحات اصلی بررسی‌شده پیدا نشد؛ ممکن است خارج از ریپو موجود باشند.

## شواهدی که داریم

| بخش | وضعیت و حدود شواهد |
|---|---|
| مرز اختیار agent و خزانه | agent فقط پیشنهاد می‌دهد؛ vault مقدارها و payload را می‌سازد و قواعد را قبل از درخواست امضا بررسی می‌کند. کلید خزانه در agent نیست. کنترل‌های owner، قفل اجرا و callbackهای شناسه‌دار در کد و تست‌ها وجود دارند. |
| معاملهٔ testnet | لاگ‌ها معاملهٔ ۲ USDC در برابر ۰٫۵۵ wNEAR و تسویهٔ دو intent در یک `execute_intents` را ثبت می‌کنند. طرف مقابل تحت کنترل تیم است. |
| امضای MPC | مدارک testnet امضای Chain Signatures را ثبت می‌کنند؛ امضا به‌تنهایی اثبات تسویه نیست. |
| رد معامله | آخرین نتیجهٔ زندهٔ خوانده‌شده `TradeTooLarge` برای پیشنهاد ۵ است؛ سقف ۵۰ دلار و notional برابر ۵۹٬۹۶۸٬۴۴۰ micro-USD. مدارک رد `InsufficientBalance` نیز وجود دارند. |
| برداشت owner | آخرین نتیجهٔ زندهٔ خوانده‌شده `Delivered` برای برداشت ۱٬۰۰۰٬۰۰۰ واحد USDC است؛ مدارک صف و اجرای پس از تأخیر واقعی ۲۴ ساعت وجود دارند. سناریوهای شکست و بازیابی زنده از این موفقیت نتیجه نمی‌شوند. |
| مدل | parser سختگیرانه، preview قواعد، ثبت پاسخ و reason hash و verifier آفلاین پیاده شده‌اند. مدل فعالِ زنده هنوز فقط `manual-v1` است. |
| UI | سایت و داشبورد read-only هستند؛ خواندن یک final block، رد پاسخ ناسازگار، محدودیت اندازه و timeout، و تفکیک sample از مشاهدهٔ زنده وجود دارند. QA بصری مرورگر در این بررسی اجرا نشد. |

معامله و تغییر دو ledger از لاگ‌های ثبت‌شده خوانده شدند؛ تراکنش تاریخی تسویه در این بررسی مستقلاً از RPC بازخوانی نشد. خواندن تازهٔ وضعیت قراردادها با موجودی نهایی و نتایج ثبت‌شده تطابق داشت، اما جای اثبات مستقل کل تاریخچه را نمی‌گیرد.

## وضعیت تازهٔ demo vault

خواندن عمومی با `python3 -B scripts/demo-readback.py` روی testnet موفق شد و تراکنشی ارسال نکرد:

| مشاهده | مقدار |
|---|---|
| Vault | `vault2.mandate-t1.testnet` |
| Verifier | `intents.mandate-t1.testnet`، fixture |
| Counterparty | `solver.mandate-t1.testnet`، تحت کنترل تیم |
| Vault USDC | ۶۷ USDC |
| Vault wNEAR | ۰٫۵۵ wNEAR |
| Counterparty USDC | ۲ USDC |
| Counterparty wNEAR | ۰٫۴۴ wNEAR |
| Pending proposal | `null` |
| Active withdrawal | `null` |
| Paused | `false` |
| Strikes | ۲ از ۳ |
| Last proposal | شمارهٔ ۵، `Rejected / TradeTooLarge` |
| Last withdrawal | شمارهٔ ۱، `Delivered / 1000000` |
| Active declared sources | `["manual-v1"]` |
| Queued declared sources | `["manual-v1", "gemini-3.1-flash-lite"]` |
| Queue maturity | ۱۲ نوامبر ۲۰۲۶، **۱۱:۰۷:۴۹ تهران**، برابر ۰۷:۳۷:۴۹ UTC |

این جدول snapshot است؛ بعد از هر عملیات دمو باید دوباره خوانده شود. اسکریپت readback هر view را با `finality: final` جدا می‌خواند؛ برخلاف reader داشبورد، همهٔ viewهای آن الزاماً متعلق به یک block ثابت نیستند.

## کارهای ضروری قبل از ارسال

### ۱. تصمیم واقعی مدل را به یک نتیجهٔ زنجیره‌ای متصل کنیم

**اولویت: ضروری برای ارائهٔ پروژه به عنوان AI agent؛ شرط دقیق track هنوز معلوم نیست.**

مدارک فعلی همه از مسیر `manual-v1` هستند. صف‌کردن اسم Gemini، تست با پاسخ ساختگی و داشتن parser، اجرای مدل واقعی را اثبات نمی‌کنند. منبع: `docs/SUBMISSION.md:21` و readback تازه.

رسیدن ساعت ۱۱:۰۷:۴۹ پذیرش را خودکار اعمال نمی‌کند. پس از maturity باید `apply_pending_mandate` اجرا شود؛ driver دستور `mandate-apply` دارد (`contracts/vault/examples/testnet_run.rs:603`). سپس `get_mandate` باید حضور دقیق اسم مدل و پاک‌شدن صف را نشان دهد.

قبل از آن تنظیمات provider، مدل، حساب agent و keeper، endpoint سازگار با chat-completions، هزینه و موجودی‌ها آماده شوند. استفادهٔ `--model --dry-run` نیز درخواست واقعی به provider می‌زند. mode مدل از نظر مبلغ و schema سختگیر است و تضمین نمی‌کند هر inference حتماً پیشنهاد قابل‌قبولی تولید کند؛ سازگاری مدل و endpoint باید عملاً تمرین شود.

معیار پایان: پاسخ واقعی provider + plan دقیق + hashهای request/response + `reason_hash` متناظر با proposal زنده + نتیجهٔ امضا یا رد روی chain. برای ادعای «معاملهٔ AI تسویه شد»، علاوه بر این‌ها final receipt تسویه و delta هر دو ledger لازم است. مدرک unsigned را با attestation یا اثبات اجرای TEE معرفی نکنیم.

### ۲. مسیر `settle` را برای معاملهٔ واقعی و موجودی فعلی آماده کنیم

**اولویت: ضروری برای تکرار دمو و تسویهٔ پیشنهاد مدل.**

در `contracts/vault/examples/testnet_run.rs:534` مقدار پرداخت counterparty ثابتِ ۰٫۵۵ wNEAR و در خط ۵۳۶ مقدار دریافت ثابتِ ۲ USDC است. مدل ممکن است مبلغ یا جهت دیگری برگزیند؛ driver فعلی برای آن ساخته نشده است.

همچنین بررسی خطوط ۵۴۷–۵۵۱ diff ساخته‌شدهٔ solver را با همان ثابت‌ها مقایسه می‌کند و diff خودِ `vault_json` را وارد مقایسه نمی‌کند. بنابراین guard با پیام «mirror of the vault» مطابقت واقعی را بررسی نمی‌کند. برای نمونه اگر vault معاملهٔ ۱ USDC در برابر ۰٫۲۷ wNEAR امضا کند، این guard همچنان intent ثابتِ ۲/۰٫۵۵ را قبول می‌کند؛ bundle ممکن است در verifier شکست بخورد. این یافته دربارهٔ driver دمو است و اثبات دورزدن قواعد vault نیست.

رفع: tuple مورد انتظار را صریح و محدود انتخاب کنیم یا diff معکوس را پس از اعتبارسنجی signer، verifier، deadline، tokenها، مقادیر و سقف بودجه از payload واقعی استخراج کنیم. بررسی باید با diff vault انجام شود، نه با ثابت‌های solver. پیش از امضای solver موجودی و حد مصرف تأیید شود. یک regression برای tuple متفاوت و یک مورد refusal لازم است.

موجودی فعلی counterparty فقط ۰٫۴۴ wNEAR است؛ معاملهٔ ثابت فعلی ۰٫۵۵ می‌خواهد و حداقل ۰٫۱۱ کسری دارد. موجودی لازم متناسب با معاملهٔ انتخابی، تعداد rehearsalها و ذخیرهٔ gas/storage آماده شود؛ funding خودکار توسط این بررسی انجام نشده است.

معیار پایان: اجرای دوبارهٔ یک معاملهٔ محدود با tuple واقعی، receipt نهایی موفق، تغییر متناظر هر دو ledger و refusal قبل از امضای solver برای tuple یا بودجهٔ نامعتبر.

### ۳. مدارک و صفحهٔ اول را با واقعیت یکسان کنیم

**اولویت: ضروری برای اعتماد و فهم داور.**

`README.md:37` هنوز funded matched settlement را باز معرفی می‌کند. `docs/PROJECT.md:121` می‌گوید جریان با پول روی chain اجرا نشده است. `web/dashboard/index.html:372` live signing، settlement و برداشت پس از delay را همگی باز معرفی می‌کند. این متن‌ها با `docs/SUBMISSION.md`، سایت اصلی و شواهد جدید ناسازگارند. تفکیک «testnet انجام شده / mainnet و recovery هنوز باز» باید در همهٔ entrypointها یکسان باشد.

در بالای README یک مسیر کوتاه برای داور لازم است: مشکل، مرز اختیار، سه نتیجهٔ اصلی، لینک سایت، لینک ویدئو، و راه اجرای بدون کلید. جزئیات فنی فعلی حفظ شوند، اما داور برای فهم دمو نباید ابتدا ۶۷۰ خط README بخواند.

`scripts/check-claims.sh` فقط وجود نام‌ها و مسیرها را در allowlist بررسی می‌کند؛ صحت ادعاهای نتیجه یا external linkها را تأیید نمی‌کند. `SUBMISSION.md` و `DEMO.md` هم در allowlist فعلی نیستند. `scripts/check-site-live.sh:20` و ۲۸ عمدتاً چند عبارت قدیمی مربوط به oracle را بررسی می‌کنند؛ پاس‌شدن آن به‌تنهایی انتشار جدول جدید یا نسخهٔ صحیح moduleها را ثابت نمی‌کند.

معیار پایان: README، PROJECT، DEMO، SUBMISSION، صفحهٔ اصلی و dashboard تاریخ و status یکسان داشته باشند؛ هر ادعای مهم به evidence مشخص و محدودیت درست وصل شود. transaction-link و فایل‌های تازهٔ ارسال دستی یا با checker مناسب بررسی شوند.

### ۴. یک release مشخص با CI موفق و سایت همان نسخه داشته باشیم

**اولویت: ضروری قبل از ارسال لینک نهایی.**

checkout فعلی `feat/site-polish` است و یک commit از `main` محلی جلوتر است. اجرای `scripts/deploy-site.sh --check` صریحاً با «production staging requires the main branch» متوقف شد. تغییر branch، merge یا انتشار توسط این بررسی انجام نشد.

snapshot عمومی GitHub، CI مربوط به commit فعلی را `in_progress` نشان داد:
<https://github.com/moltaphet/mandate-near/actions/runs/43062430295>.
برای `main` محلی، run مربوط به `7822942` در snapshot موفق بود:
<https://github.com/moltaphet/mandate-near/actions/runs/43051236170>.
جزئیات jobها و logها در این بررسی قابل‌خواندن نشدند؛ وجود یک run موفق قدیمی به معنی موفقیت release جدید نیست.

workflow فعلی هم build executor، native tests و هم `verify.sh --integration` را تعریف می‌کند. برای نسخهٔ freezeشده باید خودِ job اجرای این مراحل موفق باشد؛ پاس‌شدن تنها `docs-check` کافی نیست.

معیار پایان: commit نهایی مشخص، CI همهٔ jobهای لازم موفق، اصلاحات mergeشده، سپس سایت همان نسخه منتشر و از مرورگر عادی desktop و mobile بررسی شود: load اولیه، read vault، نمایش ledger، error/offline، replay، slider، navigation، لینک evidence و نمونهٔ synthetic. مرورگر باید درخواست‌های CORS به RPC را نیز عملاً انجام دهد. انتشار موفق CLI به‌تنهایی این‌ها را ثابت نمی‌کند.

### ۵. بستهٔ ارسال و ویدئوی دمو را نهایی کنیم

**اولویت: ضروری برای تحویل؛ الزامات رسمی باید با فرم هکاتون تطبیق داده شوند.**

`docs/SUBMISSION.md` یک متن و سناریوی سه‌دقیقه‌ای قابل‌استفاده دارد، اما لینک ویدئوی نهایی در مدارک بررسی‌شده پیدا نشد. نام/track، اعضای تیم، اطلاعات فرم و مهلت رسمی نیز تأیید نشده‌اند.

پیشنهاد ضبط: مسئله و مرز اختیار → تصمیم واقعی مدل → امضا و تسویه با delta دو ledger → رد معاملهٔ خارج از سقف → مدرک برداشت پس از ۲۴ ساعت → محدودیت‌های testnet. از explorer به‌عنوان تنها راه نشان‌دادن نتیجه استفاده نکنیم؛ readback، ledger و لاگ‌های public نیز در دسترس باشند.

معیار پایان: URL قابل‌دیدن بدون login برای دمو، لینک repository صحیح، توضیح کوتاه مطابق track، نام تیم/اعضا و همهٔ فیلدهای الزامی فرم تکمیل و یک بار از دید داور باز شوند. deck، SDK جدید یا TEE فقط اگر در مقررات واقعاً اجباری‌اند به این scope اضافه شوند.

## دو محدودیت مهم برای برنامهٔ دمو

**Circuit breaker:** vault اکنون ۲ strike دارد. رد authoritative سوم، پیشنهادها را halt می‌کند و رفع آن به `resume_proposals` توسط owner با دقیقاً یک yocto نیاز دارد (`contracts/vault/src/lib.rs:1028`). یک اجرای تأییدشدهٔ موفق strikeها را reset می‌کند؛ صرف ارسال proposal یا موفقیت settlement جای آن را نمی‌گیرد. دمو را با مسیر موفق شروع کنیم و سناریوی رد را پس از ضبط موفقیت اجرا کنیم؛ روی chain تلاش‌های نامعتبر پیاپی برای تمرین انجام ندهیم. اگر recovery از halt لازم شد، دستور و روند DAO باید از قبل آماده باشد.

**قیمت و زمان:** `max_price_age_sec` روی vault برابر ۶۰ ثانیه، proposal TTL برابر ۶۰ ثانیه و intent TTL برابر ۱۲۰ ثانیه است. `live-price-feed.py` قیمت عمومی را می‌گیرد؛ خودِ این script updater زنجیره‌ای دائمی نیست. driver مرحلهٔ جداگانهٔ `price` دارد. دریافت قیمت، گزارش به oracle، inference تا ۲۰ ثانیه، proposal، signing و settlement باید در زمان معتبر تمرین شوند. افزایش محدودیت‌ها یا جایگزینی timestamp منبع با زمان فعلی برای «راحت‌تر شدن دمو» راه‌حل محسوب نشود.

## شکاف publication برای mode مدل

`agent/src/main.rs:281` تا ۲۸۷ برای `--model` فایل model و rationale/outcome می‌نویسد، اما `proposal-<id>.json` شامل association quote تولید نمی‌کند. در مقابل `keeper/src/publication.rs:55` برای `--publish` همان فایل را الزامی می‌داند. بنابراین اتصال مستقیم model mode به relay publication با تنظیمات فعلی کامل نیست. autonomous mode این association را می‌سازد، ولی آن تصمیم‌گیری deterministic با `rebalance-v1` است.

برای ارسال فردا، مسیر settlement محدود با counterparty تحت کنترل تیم می‌تواند scope دمو باشد، پس از اصلاح driver و ثبت صادقانهٔ محدودیت. اگر ادعا می‌کنیم model → relay → solver مستقل کامل است، metadata/quote binding و شواهد live آن باید واقعاً تکمیل شوند. وجود بخش‌های جداگانهٔ model و relay این اتصال را اثبات نمی‌کند.

## بررسی‌هایی که در همین نوبت اجرا شدند

| بررسی | نتیجهٔ تازه |
|---|---|
| Formatting و strict Clippy all-targets | موفق در ابتدای `verify.sh` |
| Reference checker | موفق؛ صرفاً نام/مسیر، نه صحت ادعا |
| Intents fixture identity | ۳ تست Rust و ۴ تست Python موفق؛ verifier script هم موفق |
| Dashboard JavaScript | ۳۴ تست موفق |
| Site JavaScript | ۶ تست موفق |
| Shared-reader parity | موفق |
| Deployment inspector Python | ۱۵ تست موفق |
| Preview routing Python | ۳ تست موفق |
| Workspace library tests | vault: ۲۸۶، agent: ۸، keeper: ۸ موفق |
| Shared journal protocol | ۱۶ تست موفق |
| Executor preparation / builder Python | ۲۳ / ۲۵ تست موفق |
| Autonomous / model Rust | ۷ / ۳۹ تست موفق |
| Provenance / provenance CLI Rust | ۱۵ / ۷ تست موفق در اجرای جداگانه |
| Source-owned executor journal native | ۱۰ تست موفق روی source-v4 hash-bound |
| Journal artifact binding / export check | ۱ تست موفق؛ هیچ sandboxی در این تست شروع نمی‌شود |
| Agent protocol HTTP | ۳ موفق، ۱۱ شکست هنگام bind به localhost با EPERM؛ شکست زیرساختی |
| Keeper network HTTP | ۶ شکست هنگام bind به localhost با EPERM؛ suite بازیابی بعدی در این فرمان اجرا نشد |
| Full integration retry | اصلاً اجرا نشد؛ automatic approval review خطای داخلی داد |
| Build عادی ABI-bearing | شکست در dlopen با `mis-aligned LINKEDIT string pool` روی Mac |
| Build رسمی با `--no-abi` و WASM optimization | موفق؛ ۴۵۹٬۶۷۸ bytes |
| Production staging check | رد به علت branch غیر-main |
| Fresh public testnet readback | موفق |
| Browser / mobile / published-asset QA | اجرا نشد؛ محدودیت localhost و خطای approval |

نتیجهٔ کلی `./scripts/verify.sh` در این محیط **موفق نیست**: exit 101 به علت HTTP fixtureها. این شکست‌ها را به‌عنوان باگ قرارداد گزارش نمی‌کنیم، اما به‌عنوان تست پاس‌شده هم حساب نمی‌کنیم. ۱۱ سناریوی asynchronous journal در این بررسی اجرا نشدند. برای تعیین وضعیت خارجی آن‌ها باید log job واقعی CI نسخهٔ نهایی خوانده شود؛ متن تاریخی ۴۷۹ تست کافی نیست.

WASM تازهٔ no-ABI:
`a67b1fab3d4f68a81b89b681c05eda360404192e0fd3eae2ea961216e2625e08`.
مدرک deploy روز ۹ نوامبر hash متفاوت
`a7dec54d8f5687db29b903e2f12c91e0d5eb5b3294d9487d178be37b4cbe6fe9`
را ثبت کرده است. این اختلاف به‌تنهایی تغییر منطق یا باگ را اثبات نمی‌کند؛ metadata/build input نیز می‌تواند مؤثر باشد. تطابق بایت‌به‌بایت source جدید و deployed binary در این بررسی ادعا نمی‌شود و reproducible build اجرا نشده است.

لاگ‌های کامل محلی موقت:
`/private/tmp/mandate-review-fast-2026-11-11.log`،
`/private/tmp/mandate-review-provenance-2026-11-11.log`،
`/private/tmp/mandate-review-executor-2026-11-11.log`،
`/private/tmp/mandate-review-artifact-2026-11-11.log`،
`/private/tmp/mandate-review-keeper-2026-11-11.log`،
`/private/tmp/mandate-review-build-2026-11-11.log`،
`/private/tmp/mandate-review-wasm-2026-11-11.log`.

## ترتیب کار پیشنهادی تا فردا

1. امشب: رفع و تست driver تسویه، انتخاب بودجه/tuple دمو، آماده‌کردن تنظیمات provider و keeper، هماهنگ‌کردن مدارک و استخراج شروط واقعی فرم. صفحهٔ نهایی و مسیر خطا را در مرورگر عادی بررسی کنیم.
2. فردا بعد از ۱۱:۰۷:۴۹ تهران: اعمال صف مدل، خواندن مجدد allowlist و controls، گزارش قیمت تازه، یک اجرای محدود مدل، امضا و settlement؛ ثبت hashها و deltaها. برای این بخش حاشیهٔ زمانی برای تأخیر provider/RPC/DAO لازم است.
3. پس از ثبت مسیر موفق: سناریوی cap refusal و مدرک تاریخی برداشت را به دمو وصل کنیم؛ ویدئو و توضیح ارسال به‌روزرسانی شوند.
4. قبل از ارسال: freeze commit، CI واقعی موفق، انتشار همان نسخه و بازکردن تمام لینک‌ها بدون login. اگر آزمون مدل کامل نشد، نتیجهٔ manual را به عنوان نتیجهٔ مدل معرفی نکنیم؛ scope submission باید صادقانه اصلاح شود.

اگر deadline واقعی قبل از ۱۱:۰۷:۴۹ تهران باشد، نمی‌توان پذیرش همین صف را برای demo مدل حساب کرد. آن زمان‌بندی باید قبل از وابسته‌کردن ارسال به Gemini مشخص شود.

## قابلیت‌هایی که می‌توان بعد از هکاتون ادامه داد

برای prototype testnet، مگر آنکه track اجباری کند: solver مستقل و relay live، oracle تولیدی با updater و اعتماد بررسی‌شده، TEE/OutLayer و attestation واقعی، SHIELD، SDK مستقل، UI تراکنشی owner/agent، monitoring دائمی و نمایش تاریخچهٔ settlement.

برای real funds و mainnet همچنان بررسی مستقل امنیتی، owner governance و recovery، final-source artifact correspondence، شرایط custody و account sealing، oracle معتبر، gas/storage واقعی و سناریوهای failure/refund/recovery لازم‌اند. `docs/MAINNET-GATES.md` مبناست. یک prototype موفقِ هکاتون این شروط را خودکار برآورده نمی‌کند.

این بررسی فقط کد، فایل‌های public و دادهٔ عمومی زنجیره را خواند و تست/build محلی اجرا کرد. هیچ funding، معامله، تغییر owner policy، merge یا publication انجام نشد. تنها فایل جدید در ریپو، همین گزارش است.

**محدودیت تأیید خودکار:** درخواست اتصال مرورگر، خواندن جزئیات CI و اجرای integration خارج از محدودیت localhost با خطای داخلی `OpenAI Responses bad request ... item was created under a different OpenAI resource` در سامانهٔ approval اجرا نشدند. پیام سامانه صریحاً این را شکست review و نه تشخیص ناامن بودن عملیات می‌داند. هیچ نتیجه‌ای برای این عملیات ساخته یا فرض نشده است.
