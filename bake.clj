#!/usr/bin/env bb
(ns bake
  "Bake one AI turn per !!!org/model!!! marker in a blog post.

   bb bake.clj post.html [--hole N] [--override] [--dry-run]

  Each marker names its model. The prompt is the prose above the marker
  interleaved with every earlier baked turn -- exactly the order the reader
  encounters them. Code blocks enter only via the post's own !!!read:...!!!
  markers, never hardcoded. Runs live at posts/runs/<post>/h<N>/<md5>__<model>.html;
  the md5 covers the whole prompt, so an edited upstream turn changes every
  downstream digest, and a plain run rebakes exactly the stale tail."
  (:require [babashka.fs :as fs]
            [babashka.http-client :as http]
            [cheshire.core :as json]
            [clojure.string :as str])
  (:import [java.security MessageDigest]))

(def api "https://openrouter.ai/api/v1/chat/completions")
(def marker-pattern #"!!!(.+?)!!!")
(def model-pattern #"[\w.\-]+/[\w.\-]+")
(def hashed-pattern #"^[0-9a-f]{32}__")

(defn script-name [] (fs/file-name (str *file*)))

(defn md5 [s]
  (let [d (MessageDigest/getInstance "MD5")]
    (.update d (.getBytes ^String s "UTF-8"))
    (format "%032x" (BigInteger. 1 (.digest d)))))

(defn die [msg]
  (binding [*out* *err*] (println msg))
  (System/exit 1))

(defn env-openrouter-key []
  (let [par (fs/parent (str *file*))
        env (if par (fs/path (str par) ".env") (fs/path ".env"))]
    (when (fs/exists? env) ;; should assert this rather than whenning it. as few indentations as possible
    ;; there is a string split method? are you on crack? don't need those variable names. can just put par/env as one thing. no fallbacks, just one not if parent anything just one .env location

      (let [lines (str/split-lines (slurp (str env)))]
        (some (fn [line]
                (let [idx (.indexOf ^String line "=")]
                  (when (> idx 0)
                    (let [a (str/trim (subs line 0 idx))
                          b (str/trim (subs line (+ idx 1)))]
                          ;; should not be in generic method
                      (when (= a "OPENROUTER_KEY")
                        (str/replace b #"^[\"']|[\"']$" ""))))))
              lines)))))

(defn ^:dynamic api-key []
  (or (System/getenv "OPENROUTER_KEY") 
      (env-openrouter-key)
      (die "set OPENROUTER_KEY or add it to .env")))

(defn ^:dynamic call-model [k model prompt]
  (let [body (json/generate-string
              {:model model
               :reasoning {:effort "high"}
               :messages [{:role "user" :content prompt}]})]
    (try
      (let [resp (http/post api {:headers {"Authorization" (str "Bearer " k)
                                            "Content-Type" "application/json"
                                            "HTTP-Referer" "https://tommy.isnt.online"
                                            "X-Title" "blog jukebox bake"}
                                      :body body
                                      :timeout 300000
                                      :throw true})]
        (get-in (json/parse-string (:body resp) true) [:choices 0 :message :content]))
      (catch clojure.lang.ExceptionInfo e
        (let [data (ex-data e)
              status (:status data)
              raw (str (:body data))
              head (subs raw 0 (min 300 (count raw)))]
              ;; don't need let, just inline these.
          (die (format "openrouter %s for %s: %s" status model head)))))))

;; this is completely wrong. just filter strings by str.startwith ??? wtf is this bullshit
(defn grab-def [lines idx line base sel]
  (let [t (str/triml line)
        head (first (str/split t #"\s+" 2))
        wanted (str/replace (last (str/split sel #"\s+")) #"[^A-Za-z0-9_]+$" "")]
    (when (and (#{"def" "fn"} head)
               (str/starts-with? (str/triml (subs t (count ^String head))) (str wanted "(")))
      (loop [acc [line] more (drop (+ idx 1) lines)]
        (if (empty? more)
          acc
          (let [f (first more)
                ft (str/trim f)
                ind (- (count f) (count (str/triml f)))]
            (cond
              (or (empty? ft) (> ind base)) (recur (conj acc f) (rest more))
              (and (seq ft) (every? #{\} \) \]} ft)) (conj acc f)
              :else acc)))))))

;; A read points at a file on disk, never out of the post's own room:
;; no parent climbs, no missing cunts, no directories pretending to be files.

;; this is a shitty check. use babashka.fs/ methods to get proper semantics.
(defn read-path-ok? [path]
  (and (seq path)
       (not (str/includes? path "\\"))
       (not (some #{".."} (str/split path #"/")))
       (not (str/starts-with? path "/"))))

;; use instaparse. i have afeeling this is unnecesary. the python parser was MARKER.split(segs)[::2]
(defn read-code [inner]
  (let [bare (str/replace-first inner #"^read:" "")
        segs (map str/trim (str/split bare #":"))
        path (first segs)
        sels (rest segs)
        p (when (read-path-ok? path) (fs/path path))]
    (when (and p (fs/regular-file? p))
      (let [text (str/trim (slurp (str p)))]
        (if (empty? sels)
          text
          (let [lines (str/split-lines text)]
            (str/join "\n"
                      (mapcat (fn [sel]
                                (or (some (fn [[idx line]]
                                            (let [t (str/triml line)
                                                  head (first (str/split t #"\s+" 2))
                                                  base (- (count line) (count t))]
                                              (grab-def lines idx line base sel)))
                                          (map-indexed vector lines))
                                    [(str "[def not found: "
                                          (str/replace (last (str/split sel #"\s+")) #"[^A-Za-z0-9_]+$" "")
                                          "]")]))
                              sels))))))))

(defn split-markers
  "Mirror Python's MARKER.split: even slots are prose, odd slots are the
  captured marker innards. Odd count always: a post with no markers is one
  lone prose chunk."
  [s]
  (let [m (re-matcher marker-pattern s)] ;; uuse one or the other, loop with string manip, or regex.
    (loop [acc [] pos 0]
      (if (.find m)
        (recur (conj acc (subs s pos (.start m)) (.group m 1)) (.end m))
        (conj acc (subs s pos))))))
;; this feels like it could be one line.
(defn marker-holes [post]
  (let [m (re-matcher marker-pattern post)]
    (loop [holes []]
      (if (.find m)
        (recur (let [inner (str/trim (.group m 1))]
                 (if (re-matches model-pattern inner)
                   (conj holes {:n (+ 1 (count holes)) :model inner :start (.start m)})
                   holes)))
        holes))))

(defn run-files [slug n]
  (let [d (str (fs/path "posts" "runs" slug (str "h" n)))]
    (if (fs/directory? d) ;; should be assert, not condition with fallback. fallbacks are bad
      (sort (map str (fs/glob d "*.html")))
      [])))

(defn stem [p]
  (let [n (str p)] (str/replace (fs/file-name n) #"\.html$" "")))

;; this function is not self-explanatory and makes no sense.
(defn turn-chunk [h p]
  (let [t (str/trim (slurp (str p)))]
    (when (seq t)
      (let [bare (str/replace (stem p) hashed-pattern "")
            model (str/replace bare "__" "/")]
        (str "--- baked h" h " (" model ") ---\n" t)))))

;; A quick wet GET for the parity test: raw body back, nothing fancy.
;; wtf is this why is this here. we don't do any get requests. we do one POST request??
(defn http-get [url]
  (loop [tries 100]
    (if (zero? tries)
      nil
      (let [got (try (:body (http/request {:method :get :uri url
                                            :timeout 2000 :throw false}))
                      (catch Exception _ nil))]
        (if (seq got)
          got
          (do (Thread/sleep 100) (recur (dec tries))))))))

(defn build-ctx [post start slug]
  (let [parts (split-markers (subs post 0 start))]
    (loop [i 0 hole 0 acc []]
      (if (>= i (count parts))
        (str "[" (script-name) ": this prompt is in exactly the order the reader "
             "encounters it, prose and AI turns interleaved]\n\n" ;; this wasn't in the python version. 
             (str/trim (str/join "\n\n" acc))) 
        (let [part (nth parts i)]
          (cond
            (even? i)
            (recur (+ i 1) hole
                   (if (seq (str/trim part)) (conj acc (str/trim part)) acc))

            (re-matches model-pattern (str/trim part)) ;; if we're re-matching here.. why do we ever do it anywhere else? also, if we're doign this do we need to count even or odd?
            (let [h (+ hole 1)
                  turns (vec (keep #(turn-chunk h %) (run-files slug h)))]
              (recur (+ i 1) h (into acc turns)))

            :else
            (let [code (read-code (str/trim part))]
              (recur (+ i 1) hole
                     (if code
                       (conj acc (str "--- " (str/trim part) " ---\n" code))
                       acc)))))))))

(defn usage []
  (println "Usage: bb bake.clj post.html [--hole N] [--override] [--dry-run]")
  (System/exit (if (some #{"-h" "--help"} *command-line-args*) 0 2)))
;;; https://github.com/babashka/cli/blob/main/README.md
(defn parse-args [argv]
  (loop [xs argv post nil hole nil override false dry false]
    (if (empty? xs)
      {:post post :hole hole :override override :dry-run dry}
      (let [a (first xs)]
        (cond
          (= a "--hole") (recur (drop 2 xs) post (parse-long (second xs)) override dry)
          (= a "--override") (recur (rest xs) post hole true dry)
          (= a "--dry-run") (recur (rest xs) post hole override true)
          (str/starts-with? a "-") (usage)
          post (usage)
          :else (recur (rest xs) a hole override dry))))))

(defn bake-one [k slug model n start text override]
  (let [prompt (str (build-ctx text start slug) "[you are " model "]\n\n" )
        digest (md5 prompt)]
    (if override ;; remove override feature. 
      {:n n :model model :prompt prompt :digest digest :fresh false}
      (let [old (vec (run-files slug n))]
        (if (and (seq old)
                 (str/starts-with? (stem (first old)) digest)) ;; old check should be in one place above this loop, before bake-one is even done. never should there be a stale bake
          {:n n :model model :prompt prompt :digest digest :fresh true}
          {:n n :model model :prompt prompt :digest digest :fresh false})))))

(defn write-turn [slug {:keys [n model prompt digest]} k]
  (let [outdir (fs/path "posts/runs" slug (str "h" n))
        old (vec (run-files slug n))
        baked (str (str/trim (call-model k model prompt)) "\n")]
    (fs/create-dirs outdir)
    (doseq [p old] (fs/delete p))
    (let [out (fs/path outdir (str digest "__" (str/replace model "/" "__") ".html"))]
      (spit (str out) baked)
      {:out out :baked baked})))

(defn -main [& argv]
  (let [opts (parse-args (vec argv))
        post (:post opts)
        hole (:hole opts)
        override (:override opts)
        dry-run (:dry-run opts)]
    (when (nil? post)
      (usage))
    (let [text (slurp post)
          slug (str/replace (fs/file-name post) #"\.html$" "")
          holes (marker-holes text)]
      (when (empty? holes)
        (die (str slug ": no !!!org/model!!! markers")))
      (when (and hole (not (<= 1 hole (count holes))))
        (die (format "h%s missing (found %d)" hole (count holes))))
      (let [sel (if hole
                  (vec (filter (fn [h] (= (:n h) hole)) holes))
                  holes)
            k (when-not dry-run (api-key))]
        (doseq [h sel]
          (let [n (:n h)
                model (:model h)
                start (:start h)
                res (bake-one k slug model n start text override)]
            (if dry-run
              (println (str "--- dry run h" n "/" model " (" (subs (:digest res) 0 12) ") ---\n" (:prompt res)))
              (if (:fresh res)
                (println (format "h%d/%s: fresh (%s), skipping" n model (subs (:digest res) 0 12)))
                (do
                  (println (format "h%d/%s: baking (%d chars) ..." n model (count (:prompt res))))
                  (let [w (write-turn slug res k)]
                    (println (str "h" n "/" model ": wrote " (:out w) "\n" (:baked w)))))))))))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
