;;; qpath-test.el --- Tests for qpath.el -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Akinori Musha
;; SPDX-License-Identifier: MIT

;;; Code:

(require 'ert)
(require 'qpath)

(defmacro qpath-test--with-command (script &rest body)
  "Run BODY with `qpath-command' bound to temporary SCRIPT."
  (declare (indent 1))
  `(let ((qpath-command (make-temp-file "qpath-test-command")))
     (unwind-protect
         (progn
           (with-temp-file qpath-command
             (insert ,script))
           (set-file-modes qpath-command #o755)
           ,@body)
       (delete-file qpath-command))))

(ert-deftest qpath-read-parses-json-output ()
  (qpath-test--with-command
      "#!/bin/sh
printf '[{\"abbr\":\"i\",\"desc\":\"Init\",\"path\":\"/tmp/init.el\",\"shell_path\":\"/tmp/init.el\"}]'
"
    (let ((entries (qpath--read "file")))
      (should (= 1 (length entries)))
      (should (equal "i" (gethash "abbr" (car entries))))
      (should (equal "Init" (gethash "desc" (car entries)))))))

(ert-deftest qpath-update-refreshes-caches ()
  (qpath-test--with-command
      "#!/bin/sh
case \"$3\" in
  file)
    printf '[{\"abbr\":\"i\",\"desc\":\"Init\",\"path\":\"/tmp/init.el\",\"shell_path\":\"/tmp/init.el\"}]'
    ;;
  directory)
    printf '[{\"abbr\":\"s\",\"desc\":\"Src\",\"path\":\"/tmp/\",\"shell_path\":\"/tmp/\"}]'
    ;;
esac
"
    (let ((visit-command (symbol-function 'qpath-visit-file))
          (insert-command (symbol-function 'qpath-insert-directory))
          (qpath-visit-file-extra-sections
           '(["Version Control"
              ("=" "Show diff" ignore)]))
          qpath--file-cache
          qpath--directory-cache)
      (should (qpath-update t))
      (should (equal "i" (gethash "abbr" (car qpath--file-cache))))
      (should (equal "s" (gethash "abbr" (car qpath--directory-cache))))
      (should (eq visit-command (symbol-function 'qpath-visit-file)))
      (should (eq insert-command (symbol-function 'qpath-insert-directory))))))

(provide 'qpath-test)

;;; qpath-test.el ends here
