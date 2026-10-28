#!/usr/bin/env bb
;; Tests for bake.clj. Run from the repo root:
;;
;;   bb bake_test.clj
;;
;; Two invariants, one per test:
;; 1. The interleave: every turn lands after the prose above its marker,
;;    and nothing below the hole leaks into the prompt.
;; 2. The chain: whatever the edit/bake order, a full bake always restores
;;    a state where every file on disk is the output of the prompt that the
;;    current post and its upstream turns produce -- and a second bake is a
;;    no-op. The OpenRouter call is mocked to a hash of the prompt, so
;;    outputs change exactly when prompts do.
;; 3. Read markers expand to code, and never shift hole numbering.
;; 4. DOM/prompt parity: the published page and the model prompt carry the
;;    same chunks in the same order.
(require '[babashka.fs :as fs]
         '[babashka.http-client :as http]
         '[babashka.process :as proc]
         '[clojure.string :as str]
         '[clojure.test :as t :refer [deftest is testing]]
         '[clojure.test.check :as tc]
         '[clojure.test.check.generators :as gen]
         '[clojure.test.check.properties :as prop])

(assert (fs/exists? "bake.clj"))
(load-file "bake.clj")
(alias 'bake 'bake)

(def bang "!!!")

(defn rm-rf [p]
  (when (fs/exists? p)
    (fs/delete-tree p)))

(defn with-tmp-runs [slug f]
  (let [root (fs/path "posts/runs" slug)]
    (try (f root)
         (finally (rm-rf root)))))

(defn marker-start [post n]
  (let [m (re-matcher bake/marker-pattern post)]
    (loop [i 0]
      (when (.find m)
        (if (= i n) (.start m) (recur (+ i 1)))))))
;; Where Master's real blog room lives: the parity test fucks against the
;; real server, so its posts, runs and binary all hang off this door.
(def blog-root
  (if (fs/exists? (fs/path "target/debug/blog"))
    "."
    "/home/tommy/programming/blog"))
(defn in-blog [p] (str (fs/path blog-root p)))

;; 1. Turns interleave after their prose; nothing below the hole leaks.
(deftest interleave-order
  (testing "turns interleave in reader order, nothing below the hole leaks"
    (with-tmp-runs "__bake_selftest__"
      (fn [root]
        (fs/create-dirs (fs/path root "h1"))
        (spit (str (fs/path root "h1" "m__a.html")) "RUN-ONE")
        (fs/create-dirs (fs/path root "h2"))
        (spit (str (fs/path root "h2" "m__b.html")) "RUN-TWO")
        (let [post (str/join bang ["PROSE-ONE" "x/a" "PROSE-TWO" "x/b" "PROSE-THREE"])
              ctx (bake/build-ctx post (marker-start post 1) "__bake_selftest__")
              order (map #(.indexOf ^String ctx %) ["PROSE-ONE" "RUN-ONE" "PROSE-TWO"])]
          (is (not-any? #(= % -1) order) "not interleaved in reader order")
          (is (= order (sort order)) "not interleaved in reader order")
          (is (= -1 (.indexOf ^String ctx "RUN-TWO")) "leaks text below the hole")
          (is (= -1 (.indexOf ^String ctx "PROSE-THREE")) "leaks text below the hole")
          (is (= -1 (.indexOf ^String ctx "x/a")) "captured model ids leak as chunks")
          (is (= -1 (.indexOf ^String ctx "x/b")) "captured model ids leak as chunks"))))))

(defn digests-now [post-text slug]
  (into {}
        (comp (map (fn [h] [(:n h) (:model h) (:start h)]))
              (map (fn [[n model start]]
                     (let [prompt (str "you are " model "\n\n" (bake/build-ctx post-text start slug))]
                       [n (bake/md5 prompt)]))))
        (bake/marker-holes post-text)))

(defn consistent? [post-text slug]
  (every? (fn [[n want]]
            (let [dir (fs/path "posts/runs" slug (str "h" n))
                  files (if (fs/directory? dir) (vec (fs/glob dir "*.html")) [])]
              (and (= 1 (count files))
                   (str/starts-with? (bake/stem (first files)) want))))
          (digests-now post-text slug)))

(defn run-main [& argv]
  (with-out-str (apply bake/-main argv)))

;; 2. Whatever the edit/bake order, a full bake heals the chain.
(deftest chain
  (testing "whatever the edit/bake order, a full bake heals the chain"
    (let [cases (gen/generate
                 (gen/vector (gen/tuple (gen/elements [:e :b :h]) (gen/choose 0 99)) 1 12)
                 20 42)]
      (doseq [ops (take 8 (if (seq cases) [cases] [[[:b 1]]]))]
        (let [real-call @#'bake/call-model
              real-key @#'bake/api-key]
          (try
            (alter-var-root #'bake/call-model
                            (fn [_] (fn [_k _model prompt]
                                      (format "%064x" (BigInteger. 1 (.digest (doto (java.security.MessageDigest/getInstance "SHA-256")
                                                                               (.update (.getBytes ^String prompt "UTF-8")))))))))
            (alter-var-root #'bake/api-key (fn [_] (fn [] "quickcheck")))
            (let [slug "__quickcheck__"
                  post-path "__quickcheck__.html"
                  segs (atom ["alpha" "bravo" "charlie" "delta" "echo"])
                  step (atom 0)
                  text (fn [] (let [s @segs]
                                (str (nth s 0) bang "x/a" bang (nth s 1) bang "x/b" bang
                                     (nth s 2) bang "x/c" bang (nth s 3) bang "x/d" bang (nth s 4))))]
              (try
                (doseq [[kind i] ops]
                  (cond
                    (= kind :e) (do (swap! step inc)
                                    (swap! segs update (mod i 5) str (str " v" @step))
                                    (spit post-path (text)))
                    :else (do (spit post-path (text))
                              (if (= kind :b)
                                (do (run-main post-path)
                                    (is (consistent? (slurp post-path) slug)
                                        (str "chain broken after " ops)))
                                (run-main post-path "--hole" (str (+ 1 (mod i 4))))))))
                (run-main post-path)
                (is (consistent? (slurp post-path) slug)
                    (str "chain broken at end of " ops))
                (is (not (str/includes? (run-main post-path) "baking"))
                    (str "second bake not a no-op after " ops))
                (finally
                  (fs/delete-if-exists post-path)
                  (rm-rf (fs/path "posts/runs" slug)))))
            (finally
              (alter-var-root #'bake/call-model (fn [_] real-call))
              (alter-var-root #'bake/api-key (fn [_] real-key)))))))))

;; 3. Read markers expand to code, and never shift hole numbering.
(deftest read-expansion-and-hole-numbering
  (testing "read markers expand to code without shifting hole numbers"
    (let [src "__read_fixture.py"
          root (fs/path "posts/runs" "__read_align__")]
      (try
        (spit src "def alpha():\n    return 1\n\ndef beta():\n    return 2\n")
        (fs/create-dirs (fs/path root "h1"))
        (spit (str (fs/path root "h1" "m__a.html")) "RUN-ONE")
        (let [post (str/join bang ["P0" "read:__read_fixture.py:def beta" "P1" "x/a" "P2" "x/b" "P3"])
              ;; markers: 0 = the read, 1 = hole 1 (x/a), 2 = hole 2 (x/b).
              ;; the prompt for hole 2 starts at marker 2: everything above
              ;; it, so h1's baked turn lands after P1 and before P2.
              ctx (bake/build-ctx post (marker-start post 2) "__read_align__")
              tokens ["P0" "def beta():" "P1" "RUN-ONE" "P2"]
              pos (map #(.indexOf ^String ctx %) tokens)]
          (is (not-any? #(= % -1) pos) (str "wrong order: " (vec (map vector tokens pos))))
          (is (= pos (sort pos)) (str "wrong order: " (vec (map vector tokens pos))))
          (is (= -1 (.indexOf ^String ctx "def alpha():")) "selector leaked the unselected def")
          (is (= -1 (.indexOf ^String ctx "P3")) "leaks below the hole")
          (is (= -1 (.indexOf ^String ctx "RUN-TWO")) "leaks below the hole"))
        (finally
          (fs/delete-if-exists src)
          (rm-rf root))))))

;; 4. Integration: boot the real server and prove the published page and
;;    the model prompt are the same document.
(deftest dom-prompt-parity
  (testing "DOM and prompt carry the same chunks in the same order"
    (if (not= blog-root ".")
      (println "SKIP dom-prompt-parity: run bb bake_test.clj from the blog repo root")
      (let [binary (str (fs/path "target/debug/blog"))
            post-path "posts/20990101_parity.html"
            src "__parity_src.py"
            runs "posts/runs/20990101_parity"
            db "/tmp/parity_blog.redb"
            proc-atom (atom nil)]
        (when-not (fs/exists? binary)
          (let [r (proc/shell {:out :string :err :string} "cargo build --quiet")]
            (assert (zero? (:exit r)) (str "cargo build failed: " (:err r)))))
        (try
          (spit src "def parity_fn():\n    return 'PARITY-CODE-TOKEN'\n")
          (let [body (str/join bang ["PARITY-PROSE-1" "read:__parity_src.py"
                                     "PARITY-PROSE-2" "x/a" "PARITY-PROSE-3" "x/b" "PARITY-PROSE-4"])]
            (spit post-path (str "parity test\n" body "\n")))
          (let [full (slurp post-path)
                start1 (marker-start full 1)
                ctx1 (bake/build-ctx full start1 "20990101_parity")
                digest (bake/md5 (str "you are x/a\n\n" ctx1))]
            (fs/create-dirs (fs/path runs "h1"))
            (spit (str (fs/path runs "h1" (str digest "__x__a.html"))) "RUN-BODY-1\n"))
          (reset! proc-atom
                  (proc/process {:env (merge (into {} (System/getenv))
                                             {"PORT" "18099" "BLOG_DB" db})
                                 :out :null :err :null}
                                binary))
          (let [page (loop [tries 100]
                       (if (zero? tries)
                         nil
                         (let [got (try (:body (http/request {:method :get
                                                              :uri "http://localhost:18099/post/parity-test"
                                                              :timeout 2000 :throw false}))
                                        (catch Exception _ nil))]
                           (if (seq got) got (do (Thread/sleep 100) (recur (dec tries)))))))]
            (assert (seq page) "test server never came up")
            (let [no-script (str/replace page #"(?s)<script.*?</script>" " ")
                  visible (str/replace no-script #"<[^>]+>" " ")
                  full (slurp post-path)
                  prompt (bake/build-ctx full (marker-start full 2) "20990101_parity")
                  tokens ["PARITY-PROSE-1" "PARITY-CODE-TOKEN" "PARITY-PROSE-2"
                          "RUN-BODY-1" "PARITY-PROSE-3"]]
              (doseq [[nm text] [["DOM" visible] ["prompt" prompt]]]
                (let [pos (map #(.indexOf ^String text %) tokens)]
                  (is (not-any? #(= % -1) pos)
                      (str nm " is missing chunks: "
                           (vec (keep (fn [[t p]] (when (= p -1) t)) (map vector tokens pos)))))
                  (is (= pos (sort pos))
                      (str nm " order wrong: " (vec (map vector tokens pos))))))))
          (finally
            (when @proc-atom (proc/destroy @proc-atom))
            (fs/delete-if-exists post-path)
            (fs/delete-if-exists src)
            (rm-rf runs)
            (fs/delete-if-exists db)))))))

(defn -main [& _]
  (let [{:keys [fail error]} (t/run-tests)]
    (System/exit (if (zero? (+ fail error)) 0 1))))

(when (= *file* (System/getProperty "babashka.file"))
  (-main))
